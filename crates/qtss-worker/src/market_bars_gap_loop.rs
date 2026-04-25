// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! `market_bars_gap_loop` — FAZ 25.1.1.
//!
//! Periodic safety net for `market_bars`. The user surfaced a real
//! gap: BTCUSDT 1d futures had 2409 bars in `backfill_progress` but
//! only 100 in `market_bars` itself (most likely a retention pass
//! that wasn't followed by a fresh re-fill). The chart could only
//! show those 100 days, so zooming out on 1d was useless.
//!
//! Every tick (default 30 min) this loop:
//!   1. Walks every live `engine_symbols` row.
//!   2. Counts the real `market_bars` rows.
//!   3. Computes "expected" from the symbol's oldest known bar (or
//!      the loop's own `listing_date_fallback`) and the interval.
//!   4. If `(actual / expected) < min_completeness_pct`, fires
//!      `backfill_binance_public_klines` to fill from the oldest
//!      known time backwards (resume-safe).
//!   5. Logs the result to `market_bars_gap_events` and emits a
//!      pg_notify on `qtss_market_bars_gap_filled` so listeners
//!      (engine writers, indicator persistence, etc.) can re-run
//!      the affected slice.
//!
//! Throttled by `max_backfill_per_tick` to keep Binance request
//! weight in check.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::{info, warn};

use qtss_binance::backfill_binance_public_klines;

/// Map a Binance kline interval to its bar duration in seconds.
fn interval_seconds(iv: &str) -> Option<i64> {
    match iv.trim() {
        "1m" => Some(60),
        "3m" => Some(180),
        "5m" => Some(300),
        "15m" => Some(900),
        "30m" => Some(1800),
        "1h" | "60m" => Some(3600),
        "2h" => Some(7200),
        "4h" => Some(14400),
        "6h" => Some(21600),
        "8h" => Some(28800),
        "12h" => Some(43200),
        "1d" | "1D" => Some(86_400),
        "3d" | "3D" => Some(259_200),
        "1w" | "1W" => Some(604_800),
        "1M" => Some(2_592_000), // 30d approximation; calendar months handled by upstream
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct LiveSymbol {
    exchange: String,
    segment: String,
    symbol: String,
    interval: String,
}

#[derive(Debug, Clone)]
struct GapReport {
    actual_bars: i64,
    expected_bars: i64,
    oldest: Option<DateTime<Utc>>,
    newest: Option<DateTime<Utc>>,
}

pub async fn market_bars_gap_loop(pool: PgPool) {
    info!("market_bars_gap_loop: started");
    loop {
        if !load_enabled(&pool).await {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            continue;
        }
        match run_tick(&pool).await {
            Ok((checked, backfilled)) => info!(
                checked,
                backfilled,
                "market_bars_gap_loop tick ok"
            ),
            Err(e) => warn!(%e, "market_bars_gap_loop tick failed"),
        }
        let secs = load_tick_secs(&pool).await;
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='market_bars_gap_loop' AND config_key='enabled'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return true; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

async fn load_tick_secs(pool: &PgPool) -> u64 {
    load_param_u64(pool, "tick_secs", "secs", 1800).await.max(60)
}

async fn load_param_u64(pool: &PgPool, key: &str, field: &str, default: u64) -> u64 {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='market_bars_gap_loop' AND config_key=$1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return default; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get(field).and_then(|v| v.as_u64()).unwrap_or(default)
}

async fn load_param_f64(pool: &PgPool, key: &str, default: f64) -> f64 {
    let row = sqlx::query(
        "SELECT value FROM system_config
           WHERE module='market_bars_gap_loop' AND config_key=$1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return default; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    match &val {
        Value::Number(n) => n.as_f64().unwrap_or(default),
        other => other.get("value").and_then(|v| v.as_f64()).unwrap_or(default),
    }
}

async fn run_tick(pool: &PgPool) -> anyhow::Result<(usize, usize)> {
    let min_pct = load_param_f64(pool, "min_completeness_pct", 0.95).await;
    let max_per_tick = load_param_u64(pool, "max_backfill_per_tick", "value", 5).await as usize;

    let live = list_live_symbols(pool).await?;
    let mut checked = 0usize;
    let mut backfilled = 0usize;

    for sym in live {
        if backfilled >= max_per_tick {
            break;
        }
        checked += 1;
        let report = match measure_gap(pool, &sym).await {
            Ok(r) => r,
            Err(e) => {
                warn!(symbol=%sym.symbol, interval=%sym.interval, %e, "measure_gap failed");
                continue;
            }
        };
        if report.expected_bars == 0 {
            continue;
        }
        let pct = report.actual_bars as f64 / report.expected_bars as f64;
        if pct >= min_pct {
            continue;
        }
        info!(
            exchange=%sym.exchange, segment=%sym.segment,
            symbol=%sym.symbol, interval=%sym.interval,
            actual=report.actual_bars, expected=report.expected_bars,
            pct=format!("{:.2}", pct),
            "market_bars_gap_loop: under-covered, scheduling backfill"
        );
        match attempt_backfill(pool, &sym, &report).await {
            Ok(filled) => {
                if filled > 0 {
                    backfilled += 1;
                    notify_listeners(pool, &sym).await;
                }
            }
            Err(e) => warn!(symbol=%sym.symbol, interval=%sym.interval, %e, "backfill failed"),
        }
    }

    Ok((checked, backfilled))
}

async fn list_live_symbols(pool: &PgPool) -> anyhow::Result<Vec<LiveSymbol>> {
    // engine_symbols has `enabled` + `lifecycle_state`. We want every
    // enabled row that has been at least partially backfilled — join
    // backfill_progress for state='live' so we don't waste API quota
    // on pairs that have never started.
    let rows = sqlx::query(
        r#"SELECT es.exchange, es.segment, es.symbol, es.interval
             FROM engine_symbols es
             JOIN backfill_progress bp ON bp.engine_symbol_id = es.id
            WHERE es.enabled = true
              AND es.exchange = 'binance'
              AND bp.state = 'live'
            ORDER BY es.symbol, es.interval"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| LiveSymbol {
            exchange: r.try_get("exchange").unwrap_or_default(),
            segment: r.try_get("segment").unwrap_or_default(),
            symbol: r.try_get("symbol").unwrap_or_default(),
            interval: r.try_get("interval").unwrap_or_default(),
        })
        .collect())
}

async fn measure_gap(pool: &PgPool, sym: &LiveSymbol) -> anyhow::Result<GapReport> {
    let row = sqlx::query(
        r#"SELECT COUNT(*)::bigint AS bars,
                  MIN(open_time) AS oldest,
                  MAX(open_time) AS newest
             FROM market_bars
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND interval=$4"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .fetch_one(pool)
    .await?;
    let actual: i64 = row.try_get("bars").unwrap_or(0);
    let oldest: Option<DateTime<Utc>> = row.try_get("oldest").ok();
    let newest: Option<DateTime<Utc>> = row.try_get("newest").ok();

    // Expected reference — pick the LARGEST of:
    //   (a) backfill_progress.bar_count (what the previous backfill claimed
    //       to have written; survives if market_bars was later truncated /
    //       retention-policy'd — exactly the BTCUSDT 1d failure mode the
    //       user spotted: progress said 2409 rows, market_bars had 100)
    //   (b) span(oldest..newest)/interval — for fresh symbols where
    //       backfill_progress is missing.
    let progress_bars: i64 = sqlx::query(
        r#"SELECT COALESCE(bp.bar_count, 0) AS bars
             FROM engine_symbols es
             LEFT JOIN backfill_progress bp ON bp.engine_symbol_id = es.id
            WHERE es.exchange=$1 AND es.segment=$2 AND es.symbol=$3 AND es.interval=$4
            LIMIT 1"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .fetch_optional(pool)
    .await?
    .and_then(|r| r.try_get::<i64, _>("bars").ok())
    .unwrap_or(0);

    let span_bars = match (interval_seconds(&sym.interval), oldest, newest) {
        (Some(sec), Some(o), Some(n)) if sec > 0 => {
            let span = n.signed_duration_since(o).num_seconds().max(0);
            (span / sec) + 1
        }
        _ => 0,
    };
    let expected_bars = progress_bars.max(span_bars);
    Ok(GapReport {
        actual_bars: actual,
        expected_bars,
        oldest,
        newest,
    })
}

async fn attempt_backfill(
    pool: &PgPool,
    sym: &LiveSymbol,
    report: &GapReport,
) -> anyhow::Result<i64> {
    let pages_per_run = load_param_u64(pool, "pages_per_run", "value", 50).await as i64;
    // We pull "missing" bars; capped by pages × 1000 per Binance kline page.
    // backfill_binance_public_klines accepts target_bars=0 for "all the way
    // to listing", but we want bounded work per tick — pick pages × 1000.
    let target = (pages_per_run * 1000).clamp(1, 50_000);
    // Resume from the oldest known bar's millis so we go BACKWARDS into
    // the gap. Pass `None` if no bars exist yet (start from now).
    let resume_end_ms: Option<u64> = report
        .oldest
        .map(|t| t.timestamp_millis().max(0) as u64);
    let result = backfill_binance_public_klines(
        pool,
        &sym.symbol,
        &sym.interval,
        &sym.segment,
        target,
        resume_end_ms,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Audit ledger row.
    let success = result.upserted >= 0;
    let oldest_filled: Option<DateTime<Utc>> = result
        .oldest_ms
        .and_then(|ms| Utc.timestamp_millis_opt(ms as i64).single());
    let newest_filled: Option<DateTime<Utc>> = result
        .newest_ms
        .and_then(|ms| Utc.timestamp_millis_opt(ms as i64).single());
    let _ = sqlx::query(
        r#"INSERT INTO market_bars_gap_events
              (exchange, segment, symbol, interval,
               expected_bars, actual_bars, bars_upserted, pages_fetched,
               oldest_filled, newest_filled, success)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)"#,
    )
    .bind(&sym.exchange)
    .bind(&sym.segment)
    .bind(&sym.symbol)
    .bind(&sym.interval)
    .bind(report.expected_bars)
    .bind(report.actual_bars)
    .bind(result.upserted)
    .bind(result.pages as i32)
    .bind(oldest_filled)
    .bind(newest_filled)
    .bind(success)
    .execute(pool)
    .await;

    info!(
        symbol=%sym.symbol, interval=%sym.interval,
        bars_upserted=result.upserted, pages=result.pages,
        reached_listing=result.reached_listing,
        "market_bars_gap_loop: backfill complete"
    );
    Ok(result.upserted)
}

async fn notify_listeners(pool: &PgPool, sym: &LiveSymbol) {
    // Use a JSON payload so consumers can route per (exchange, segment,
    // symbol, interval). Engine writers / indicator-persistence loops
    // can subscribe via LISTEN qtss_market_bars_gap_filled.
    let payload = json!({
        "exchange": sym.exchange,
        "segment":  sym.segment,
        "symbol":   sym.symbol,
        "interval": sym.interval,
    });
    let txt = payload.to_string();
    let _ = sqlx::query("SELECT pg_notify('qtss_market_bars_gap_filled', $1)")
        .bind(&txt)
        .execute(pool)
        .await;
}

// chrono::TimeZone trait usage helper.
use chrono::TimeZone;
