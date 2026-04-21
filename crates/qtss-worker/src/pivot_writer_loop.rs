//! Canonical pivot persistence loop.
//!
//! Every tick (default 60s):
//!   * Walk `engine_symbols WHERE enabled = true`.
//!   * For each series, pull the most recent ~2k bars from `market_bars`.
//!   * Run `qtss_pivots::zigzag::compute_pivots` across five Fibonacci
//!     slots (3/5/8/13/21 — overridable via `system_config.zigzag.slot_*`).
//!   * Upsert each confirmed pivot into `pivots`, attaching an HH/HL/LL/LH
//!     `swing_tag` relative to the prior same-direction pivot on that
//!     level.
//!
//! This is the single write path the API `GET /v2/zigzag/...` endpoint
//! will read from (Faz 6.b — follow-up). Until the read fallback lands,
//! the endpoint still live-computes on demand; the `pivots` table simply
//! accumulates a durable history for downstream consumers (detectors
//! reading HH/HL tags, audits, ML features).

use std::time::Duration;

use qtss_pivots::zigzag::{compute_pivots, Sample};
use qtss_storage::{market_bars, resolve_worker_enabled_flag, resolve_worker_tick_secs};
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use tracing::{debug, info, warn};

pub async fn pivot_writer_loop(pool: PgPool) {
    info!("pivot_writer_loop started");
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "worker",
            "pivot_writer_enabled",
            "QTSS_PIVOT_WRITER_ENABLED",
            true,
        )
        .await;

        if enabled {
            match run_once(&pool).await {
                Ok(stats) => info!(
                    series = stats.series_processed,
                    rows = stats.rows_upserted,
                    "pivot_writer ok"
                ),
                Err(e) => warn!(%e, "pivot_writer failed"),
            }
        }

        let secs = resolve_worker_tick_secs(
            &pool,
            "worker",
            "pivot_writer_tick_secs",
            "QTSS_PIVOT_WRITER_TICK_SECS",
            60,
            15,
        )
        .await;
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

#[derive(Default)]
struct Stats {
    series_processed: usize,
    rows_upserted: usize,
}

#[derive(Debug)]
struct SymbolRow {
    id: sqlx::types::Uuid,
    exchange: String,
    segment: String,
    symbol: String,
    interval: String,
}

async fn list_enabled_symbols(pool: &PgPool) -> anyhow::Result<Vec<SymbolRow>> {
    let rows = sqlx::query(
        r#"SELECT id, exchange, segment, symbol, "interval"
             FROM engine_symbols
            WHERE enabled = true
            ORDER BY exchange, segment, symbol, "interval""#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| SymbolRow {
            id: r.get("id"),
            exchange: r.get("exchange"),
            segment: r.get("segment"),
            symbol: r.get("symbol"),
            interval: r.get("interval"),
        })
        .collect())
}

#[derive(Debug)]
struct SlotCfg {
    length: u32,
}

async fn load_slot_lengths(pool: &PgPool) -> [u32; 5] {
    let defaults: [u32; 5] = [3, 5, 8, 13, 21];
    let mut out = defaults;
    for i in 0..5usize {
        let key = format!("slot_{i}");
        if let Ok(Some(row)) = sqlx::query(
            "SELECT value FROM system_config WHERE module = 'zigzag' AND config_key = $1",
        )
        .bind(&key)
        .fetch_optional(pool)
        .await
        {
            let val: serde_json::Value = row.try_get("value").unwrap_or(serde_json::Value::Null);
            if let Some(len) = val.get("length").and_then(|v| v.as_u64()) {
                out[i] = (len.max(1)) as u32;
            }
        }
    }
    out
}

fn swing_tag_for(direction: i8, price: Decimal, prev: Option<&(i8, Decimal)>) -> Option<&'static str> {
    let prev = prev?;
    if prev.0 != direction {
        return None;
    }
    if direction == 1 {
        Some(if price >= prev.1 { "HH" } else { "LH" })
    } else {
        Some(if price <= prev.1 { "LL" } else { "HL" })
    }
}

async fn process_symbol(
    pool: &PgPool,
    sym: &SymbolRow,
    slot_lengths: &[u32; 5],
) -> anyhow::Result<usize> {
    let bars = market_bars::list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        2000,
    )
    .await?;
    if bars.len() < 10 {
        return Ok(0);
    }
    // DB gave us newest-first; reverse to chronological for the port.
    let mut chrono_bars: Vec<_> = bars.into_iter().rev().collect();
    let samples: Vec<Sample> = chrono_bars
        .iter_mut()
        .enumerate()
        .map(|(i, r)| Sample {
            bar_index: i as u64,
            time: r.open_time,
            high: r.high,
            low: r.low,
            volume: r.volume,
        })
        .collect();

    let mut written = 0usize;
    for (slot_idx, length) in slot_lengths.iter().enumerate() {
        let all = compute_pivots(&samples, *length);
        // Drop the running head — it may still drift to a later bar on
        // future bars (Pine's `not dirchanged` replace path), which
        // would leave stale rows in `pivots` keyed by the old open_time.
        // Only pivots locked in by a subsequent opposite-direction pivot
        // are safe to persist.
        let confirmed: &[_] = if all.is_empty() { &all } else { &all[..all.len() - 1] };
        let mut prev_same: Option<(i8, Decimal)> = None;
        for cp in confirmed {
            // Pine-style direction: ±1 (LH/HL/equal/first) or ±2 (HH/LL).
            let direction: i8 = cp.direction;
            let swing = swing_tag_for(direction.signum(), cp.price, prev_same.as_ref());
            prev_same = Some((direction.signum(), cp.price));
            sqlx::query(
                r#"INSERT INTO pivots
                      (engine_symbol_id, level, bar_index, open_time,
                       direction, price, volume, swing_tag, prominence)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                   ON CONFLICT (engine_symbol_id, level, open_time) DO UPDATE
                      SET bar_index   = EXCLUDED.bar_index,
                          direction   = EXCLUDED.direction,
                          price       = EXCLUDED.price,
                          volume      = EXCLUDED.volume,
                          swing_tag   = EXCLUDED.swing_tag,
                          prominence  = EXCLUDED.prominence,
                          computed_at = now()"#,
            )
            .bind(sym.id)
            .bind(slot_idx as i16)
            .bind(cp.bar_index as i64)
            .bind(cp.time)
            .bind(direction as i16)
            .bind(cp.price)
            .bind(cp.volume_at_pivot)
            .bind(swing)
            .bind(cp.prominence)
            .execute(pool)
            .await?;
            written += 1;
        }
    }
    Ok(written)
}

async fn run_once(pool: &PgPool) -> anyhow::Result<Stats> {
    let mut stats = Stats::default();
    let symbols = list_enabled_symbols(pool).await?;
    let slot_lengths = load_slot_lengths(pool).await;
    for sym in symbols {
        match process_symbol(pool, &sym, &slot_lengths).await {
            Ok(n) => {
                stats.series_processed += 1;
                stats.rows_upserted += n;
                debug!(
                    sym = %format!("{}/{}/{}", sym.exchange, sym.symbol, sym.interval),
                    rows = n,
                    "pivot_writer: upserted"
                );
            }
            Err(e) => {
                warn!(
                    sym = %format!("{}/{}/{}", sym.exchange, sym.symbol, sym.interval),
                    %e,
                    "pivot_writer: symbol failed"
                );
            }
        }
    }
    Ok(stats)
}

// `SlotCfg` currently unused but left as an anchor for a richer per-slot
// config shape (e.g. per-symbol overrides) without touching the loop.
#[allow(dead_code)]
fn _unused_slot_cfg_anchor() -> SlotCfg {
    SlotCfg { length: 3 }
}
