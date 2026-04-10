//! `engine_symbols` → `market_bars` coverage: REST history + health/gap metrics (`engine_symbol_ingestion_state`).

use std::time::Duration;

use chrono::{DateTime, Utc};
use qtss_binance::backfill_binance_public_klines;
use qtss_storage::{
    count_market_bars_series, is_binance_futures_tradable, list_enabled_engine_symbols,
    list_recent_bar_open_times_desc, resolve_system_u64, resolve_worker_tick_secs,
    upsert_engine_symbol_ingestion_state, EngineSymbolRow,
};
use sqlx::PgPool;
use tracing::{debug, info, warn};

fn binance_kline_interval_seconds(iv: &str) -> Option<i64> {
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
        _ => None,
    }
}

fn segment_db(segment: &str) -> &'static str {
    match segment.trim().to_lowercase().as_str() {
        "futures" | "usdt_futures" | "fapi" => "futures",
        _ => "spot",
    }
}

/// Count gaps in ascending time order: consecutive step differs >15% from expected bar length.
fn scan_gaps_asc(times_asc: &[DateTime<Utc>], expected_sec: i64) -> (i32, Option<i32>) {
    if times_asc.len() < 2 || expected_sec <= 0 {
        return (0, None);
    }
    let tol = (expected_sec as f64 * 0.15).max(2.0);
    let mut gaps = 0_i32;
    let mut max_excess: i64 = 0;
    for w in times_asc.windows(2) {
        let dt = w[1].signed_duration_since(w[0]).num_seconds();
        if dt <= 0 {
            gaps += 1;
            continue;
        }
        let diff = (dt - expected_sec).abs();
        if diff as f64 > tol {
            gaps += 1;
            max_excess = max_excess.max(dt - expected_sec);
        }
    }
    let max_gap_i32 = if max_excess > 0 {
        Some(max_excess.clamp(0, i32::MAX as i64) as i32)
    } else {
        None
    };
    (gaps, max_gap_i32)
}

async fn run_one_target(
    pool: &PgPool,
    row: &EngineSymbolRow,
    min_bars: i64,
    gap_window: i64,
) {
    let ex = row.exchange.trim();
    let seg = row.segment.trim();
    let sym = row.symbol.trim();
    let iv = row.interval.trim();
    let now = Utc::now();

    if !ex.eq_ignore_ascii_case("binance") {
        let _ = upsert_engine_symbol_ingestion_state(
            pool,
            row.id,
            0,
            None,
            None,
            0,
            None,
            None,
            now,
            Some("non_binance_auto_ingest_not_supported"),
        )
        .await;
        return;
    }

    if segment_db(seg) == "futures" {
        let tradable = is_binance_futures_tradable(pool, sym).await.unwrap_or(false);
        if !tradable {
            debug!(
                engine_symbol_id = %row.id,
                symbol = %sym,
                "engine_ingest: skip backfill — symbol not Binance USDT-M tradable (catalog)"
            );
            let _ = upsert_engine_symbol_ingestion_state(
                pool,
                row.id,
                0,
                None,
                None,
                0,
                None,
                None,
                now,
                Some("not_binance_usdt_m_tradable"),
            )
            .await;
            return;
        }
    }

    let (count, min_ot, max_ot) = match count_market_bars_series(pool, ex, segment_db(seg), sym, iv).await {
        Ok(x) => x,
        Err(e) => {
            warn!(%e, engine_symbol_id = %row.id, "count_market_bars_series");
            let _ = upsert_engine_symbol_ingestion_state(
                pool,
                row.id,
                0,
                None,
                None,
                0,
                None,
                None,
                now,
                Some("count_market_bars_failed"),
            )
            .await;
            return;
        }
    };

    let mut last_backfill: Option<chrono::DateTime<Utc>> = None;
    let mut last_err: Option<String> = None;

    if count < min_bars {
        let need = (min_bars - count).max(0) as i64;
        let fetch_n = need.saturating_add(200).clamp(300, 15_000);
        match backfill_binance_public_klines(pool, sym, iv, seg, fetch_n).await {
            Ok(n) => {
                last_backfill = Some(Utc::now());
                info!(
                    engine_symbol_id = %row.id,
                    symbol = %sym,
                    interval = %iv,
                    upserted = n,
                    "engine_ingest REST backfill"
                );
            }
            Err(e) => {
                last_err = Some(format!("backfill:{e}"));
                warn!(%e, engine_symbol_id = %row.id, symbol = %sym, "engine_ingest backfill");
            }
        }
    }

    let (count2, min_ot2, max_ot2) = match count_market_bars_series(pool, ex, segment_db(seg), sym, iv).await {
        Ok(x) => x,
        Err(_) => (count, min_ot, max_ot),
    };

    let exp = binance_kline_interval_seconds(iv);
    let mut gap_n = 0;
    let mut max_gap_sec: Option<i32> = None;
    if let Some(sec) = exp {
        match list_recent_bar_open_times_desc(pool, ex, segment_db(seg), sym, iv, gap_window).await {
            Ok(mut desc) => {
                desc.reverse();
                let (g, mx) = scan_gaps_asc(&desc, sec);
                gap_n = g;
                max_gap_sec = mx;
            }
            Err(e) => warn!(%e, "list_recent_bar_open_times_desc"),
        }
    }

    // Self-healing patch backfill — runs whenever the cold-start branch
    // didn't fire. Two triggers, both pointing at the same fix:
    //   1. Stale tail: now - max_ot is more than 2× the bar interval,
    //      meaning the live feed has been silent (worker downtime, WS
    //      reconnect storm, venue outage). Catch up from the gap.
    //   2. Interior holes: scan_gaps_asc reported gap_n > 0; the gap
    //      window may stretch back farther than the stale-tail measure,
    //      so we cover it explicitly.
    // The Binance backfill helper takes "last N bars" rather than a
    // since-timestamp; we size N to comfortably cover whichever trigger
    // fired and rely on the (exchange, segment, symbol, interval,
    // open_time) UPSERT to make the re-fetch idempotent.
    let needs_patch = exp
        .and_then(|sec| {
            let stale_n = max_ot2.map(|t| {
                ((now.signed_duration_since(t).num_seconds() / sec).max(0) + 50) as i64
            });
            let gap_n_fetch = if gap_n > 0 { Some(gap_window) } else { None };
            match (stale_n, gap_n_fetch) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            }
            .filter(|n| {
                // Only fire when we'd actually fetch *more* than the
                // cold-start branch already did this tick.
                last_backfill.is_none()
                    && exp
                        .map(|sec| {
                            max_ot2
                                .map(|t| now.signed_duration_since(t).num_seconds() > sec * 2)
                                .unwrap_or(true)
                                || gap_n > 0
                        })
                        .unwrap_or(false)
                    && *n >= 60
            })
        })
        .map(|n| n.clamp(300, 15_000));

    if let Some(fetch_n) = needs_patch {
        match backfill_binance_public_klines(pool, sym, iv, seg, fetch_n).await {
            Ok(n) => {
                last_backfill = Some(Utc::now());
                info!(
                    engine_symbol_id = %row.id,
                    symbol = %sym,
                    interval = %iv,
                    upserted = n,
                    gap_n,
                    "engine_ingest patch backfill (gap/stale)"
                );
                // Re-scan after the patch so the metrics row reflects
                // the post-fix state instead of the pre-fix one.
                if let Ok(x) = count_market_bars_series(pool, ex, segment_db(seg), sym, iv).await {
                    let (c3, mn3, mx3) = x;
                    if let (Some(sec), Ok(mut desc)) = (
                        exp,
                        list_recent_bar_open_times_desc(pool, ex, segment_db(seg), sym, iv, gap_window).await,
                    ) {
                        desc.reverse();
                        let (g, mx) = scan_gaps_asc(&desc, sec);
                        gap_n = g;
                        max_gap_sec = mx;
                    }
                    return upsert_engine_symbol_ingestion_state(
                        pool,
                        row.id,
                        c3.clamp(0, i32::MAX as i64) as i32,
                        mn3,
                        mx3,
                        gap_n,
                        max_gap_sec,
                        last_backfill,
                        now,
                        None,
                    )
                    .await
                    .map(|_| ())
                    .unwrap_or(());
                }
            }
            Err(e) => {
                warn!(%e, engine_symbol_id = %row.id, symbol = %sym, "engine_ingest patch backfill");
                last_err = Some(format!("patch_backfill:{e}"));
            }
        }
    }

    let mut err = last_err;
    if err.is_none() {
        if let (Some(maxt), Some(sec)) = (max_ot2, exp) {
            let age = now.signed_duration_since(maxt).num_seconds();
            if age > sec.saturating_mul(6) {
                err = Some(format!("stale_feed_seconds:{age}"));
            }
        }
    }

    let _ = upsert_engine_symbol_ingestion_state(
        pool,
        row.id,
        count2.clamp(0, i32::MAX as i64) as i32,
        min_ot2,
        max_ot2,
        gap_n,
        max_gap_sec,
        last_backfill,
        now,
        err.as_deref(),
    )
    .await;
}

pub async fn engine_symbol_ingest_loop(pool: PgPool) {
    loop {
        let tick = resolve_worker_tick_secs(
            &pool,
            "worker",
            "engine_ingest_tick_secs",
            "QTSS_ENGINE_INGEST_TICK_SECS",
            180,
            60,
        )
        .await;
        let min_bars = resolve_system_u64(
            &pool,
            "worker",
            "engine_ingest_min_bars",
            "QTSS_ENGINE_INGEST_MIN_BARS",
            2_000,
            120,
            50_000,
        )
        .await as i64;
        let gap_window = resolve_system_u64(
            &pool,
            "worker",
            "engine_ingest_gap_window",
            "QTSS_ENGINE_INGEST_GAP_WINDOW",
            2_000,
            100,
            20_000,
        )
        .await as i64;

        match list_enabled_engine_symbols(&pool).await {
            Ok(rows) => {
                for r in rows {
                    run_one_target(&pool, &r, min_bars, gap_window).await;
                }
            }
            Err(e) => warn!(%e, "engine_ingest list_enabled_engine_symbols"),
        }
        tokio::time::sleep(Duration::from_secs(tick)).await;
    }
}
