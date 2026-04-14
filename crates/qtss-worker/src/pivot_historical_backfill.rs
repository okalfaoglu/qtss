//! Pivot historical backfill — Faz 10 / P2a.
//!
//! Shared infrastructure for every consumer of `pivot_cache` (Wyckoff,
//! Elliott, Classical, Harmonic). The v2_detection_orchestrator rebuilds
//! the PivotEngine every tick from the last `history_bars` bars, which
//! means pivots older than that window never enter the cache *and* the
//! `bar_index` written under the rolling window is inconsistent with a
//! true global-position index.
//!
//! This loop fixes both:
//!   1. Iterate every stored bar for each enabled symbol × timeframe,
//!      chronologically from the first, through a single PivotEngine.
//!      bar_index is therefore GLOBAL (position in the full series).
//!   2. On completion, wipe any prior cache rows for the series and
//!      write the full L0..L3 pivot set.
//!   3. Record the cursor (last_open_time, bars_processed, pivots
//!      written) in `pivot_backfill_state` so subsequent ticks only
//!      rebuild when new bars accumulate past it.
//!
//! Why full rebuild vs. incremental? ATR + zigzag legs are stateful,
//! and accurate replay requires starting from bar 0. The worker fires
//! hourly by default — acceptable cost for global correctness, because
//! downstream detectors otherwise produce phase-A-invisible setups.
//!
//! CLAUDE.md compliance:
//!   - #1 (no scattered if/else): single loop over enabled symbols,
//!     branch-by-guard with early `continue`s; per-level work folded
//!     through a fixed `LEVELS` table.
//!   - #2 (config-driven): enable flag, tick interval, chunk size, and
//!     minimum bar count all read from system_config.

use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::pivot::{PivotKind, PivotLevel};
use qtss_domain::v2::timeframe::Timeframe;
use qtss_pivots::{PivotConfig, PivotEngine};
use qtss_storage::{
    delete_pivot_cache_for_series, get_pivot_backfill_state, list_bars_after_asc,
    list_enabled_engine_symbols, resolve_system_u64, resolve_worker_enabled_flag,
    resolve_worker_tick_secs, upsert_pivot_backfill_state, upsert_pivot_cache_batch,
    EngineSymbolRow, PivotBackfillState, PivotCacheRow,
};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::{debug, info, warn};

const LEVELS: [(PivotLevel, &str); 4] = [
    (PivotLevel::L0, "L0"),
    (PivotLevel::L1, "L1"),
    (PivotLevel::L2, "L2"),
    (PivotLevel::L3, "L3"),
];

pub async fn pivot_historical_backfill_loop(pool: PgPool) {
    info!("pivot_historical_backfill_loop started");
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "worker",
            "pivot_backfill_enabled",
            "QTSS_PIVOT_BACKFILL_ENABLED",
            true,
        )
        .await;
        let tick_secs = resolve_worker_tick_secs(
            &pool,
            "worker",
            "pivot_backfill_tick_secs",
            "QTSS_PIVOT_BACKFILL_TICK_SECS",
            3600,
            60,
        )
        .await;

        if !enabled {
            tokio::time::sleep(Duration::from_secs(tick_secs)).await;
            continue;
        }

        if let Err(e) = run_pass(&pool).await {
            warn!(%e, "pivot_historical_backfill pass failed");
        }

        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
    }
}

async fn run_pass(pool: &PgPool) -> anyhow::Result<()> {
    let chunk_bars =
        resolve_system_u64(pool, "detector", "pivot_backfill.chunk_bars", "", 5_000, 500, 50_000)
            .await as i64;
    let min_bars =
        resolve_system_u64(pool, "detector", "pivot_backfill.min_bars", "", 60, 10, 10_000).await
            as usize;

    let symbols = list_enabled_engine_symbols(pool).await?;
    let mut processed = 0u32;
    let mut skipped = 0u32;
    for sym in &symbols {
        match backfill_symbol(pool, sym, chunk_bars, min_bars).await {
            Ok(true) => processed += 1,
            Ok(false) => skipped += 1,
            Err(e) => warn!(
                symbol = %sym.symbol, interval = %sym.interval, %e,
                "pivot backfill failed for symbol"
            ),
        }
    }
    if processed > 0 {
        info!(
            processed,
            skipped,
            total = symbols.len(),
            "pivot_historical_backfill pass complete"
        );
    } else {
        debug!(skipped, total = symbols.len(), "pivot_historical_backfill: nothing to do");
    }
    Ok(())
}

/// Returns `Ok(true)` if we actually rebuilt, `Ok(false)` if skipped.
async fn backfill_symbol(
    pool: &PgPool,
    sym: &EngineSymbolRow,
    chunk_bars: i64,
    min_bars: usize,
) -> anyhow::Result<bool> {
    // Never run on partially-ingested series — we'd bake incomplete
    // pivots into the global index and have to rebuild again later.
    if !qtss_storage::is_backfill_ready(pool, sym.id).await {
        return Ok(false);
    }

    let timeframe = match parse_timeframe(&sym.interval) {
        Some(tf) => tf,
        None => {
            debug!(interval = %sym.interval, "skip: unsupported timeframe");
            return Ok(false);
        }
    };

    // Peek: does the newest bar exceed our cursor?
    let newest_bar =
        qtss_storage::list_recent_bars(pool, &sym.exchange, &sym.segment, &sym.symbol, &sym.interval, 1)
            .await?;
    let newest_open_time = match newest_bar.first() {
        Some(b) => b.open_time,
        None => return Ok(false), // no bars yet
    };

    let prior_state =
        get_pivot_backfill_state(pool, &sym.exchange, &sym.segment, &sym.symbol, &sym.interval)
            .await?;
    if let Some(s) = prior_state.as_ref() {
        if s.last_open_time >= newest_open_time {
            return Ok(false); // up-to-date
        }
    }

    info!(
        symbol = %sym.symbol, interval = %sym.interval,
        prior = ?prior_state.as_ref().map(|s| s.last_open_time),
        newest = %newest_open_time,
        "pivot backfill: starting full rebuild"
    );

    let instrument = build_instrument(&sym.exchange, &sym.segment, &sym.symbol);
    let mut engine = PivotEngine::new(PivotConfig::defaults())?;

    // Accumulate emitted pivots per level in memory. Each level holds at
    // most one pivot per raw pivot confirmation, bounded by the series
    // length — tractable even for years of history on liquid symbols.
    let mut rows_by_level: [Vec<PivotCacheRow>; 4] = Default::default();

    let mut cursor: DateTime<Utc> = Utc.timestamp_opt(0, 0).unwrap();
    let mut bars_processed: i64 = 0;
    let mut last_bar_time: DateTime<Utc> = cursor;

    loop {
        let chunk = list_bars_after_asc(
            pool,
            &sym.exchange,
            &sym.segment,
            &sym.symbol,
            &sym.interval,
            cursor,
            chunk_bars,
        )
        .await?;
        if chunk.is_empty() {
            break;
        }
        for row in &chunk {
            let bar = Bar {
                instrument: instrument.clone(),
                timeframe,
                open_time: row.open_time,
                open: row.open,
                high: row.high,
                low: row.low,
                close: row.close,
                volume: row.volume,
                closed: true,
            };
            let bar_idx_before = bars_processed;
            match engine.on_bar(&bar) {
                Ok(new_pivots) => {
                    for np in new_pivots {
                        let level_idx = match np.level {
                            PivotLevel::L0 => 0,
                            PivotLevel::L1 => 1,
                            PivotLevel::L2 => 2,
                            PivotLevel::L3 => 3,
                        };
                        rows_by_level[level_idx].push(PivotCacheRow {
                            exchange: sym.exchange.clone(),
                            symbol: sym.symbol.clone(),
                            timeframe: sym.interval.clone(),
                            level: LEVELS[level_idx].1.to_string(),
                            bar_index: np.pivot.bar_index as i64,
                            open_time: np.pivot.time,
                            price: np.pivot.price,
                            kind: match np.pivot.kind {
                                PivotKind::High => "High".to_string(),
                                PivotKind::Low => "Low".to_string(),
                            },
                            prominence: np.pivot.prominence,
                            volume_at_pivot: np.pivot.volume_at_pivot,
                            swing_type: np.pivot.swing_type.map(|s| format!("{:?}", s)),
                        });
                    }
                }
                Err(e) => {
                    warn!(
                        symbol = %sym.symbol, interval = %sym.interval, bar_idx_before, %e,
                        "pivot engine rejected bar during backfill — aborting this series"
                    );
                    return Ok(false);
                }
            }
            bars_processed += 1;
            last_bar_time = row.open_time;
        }
        cursor = chunk.last().unwrap().open_time;
        if (chunk.len() as i64) < chunk_bars {
            break;
        }
    }

    if (bars_processed as usize) < min_bars {
        debug!(
            symbol = %sym.symbol, interval = %sym.interval, bars_processed,
            "pivot backfill skip: below min_bars threshold"
        );
        return Ok(false);
    }

    // Wipe old inconsistent cache and rewrite atomically from a single
    // engine. Per-level batch keeps the transaction small.
    let deleted =
        delete_pivot_cache_for_series(pool, &sym.exchange, &sym.symbol, &sym.interval).await?;
    let mut total_written: u64 = 0;
    for rows in &rows_by_level {
        if rows.is_empty() {
            continue;
        }
        total_written += upsert_pivot_cache_batch(pool, rows).await?;
    }

    upsert_pivot_backfill_state(
        pool,
        &PivotBackfillState {
            exchange: sym.exchange.clone(),
            segment: sym.segment.clone(),
            symbol: sym.symbol.clone(),
            timeframe: sym.interval.clone(),
            last_open_time: last_bar_time,
            bars_processed,
            pivots_written: total_written as i64,
        },
    )
    .await?;

    info!(
        symbol = %sym.symbol,
        interval = %sym.interval,
        bars = bars_processed,
        deleted_rows = deleted,
        wrote = total_written,
        l0 = rows_by_level[0].len(),
        l1 = rows_by_level[1].len(),
        l2 = rows_by_level[2].len(),
        l3 = rows_by_level[3].len(),
        "pivot backfill: series rebuilt"
    );
    Ok(true)
}

fn build_instrument(exchange: &str, _segment: &str, symbol: &str) -> Instrument {
    let venue = match exchange.to_lowercase().as_str() {
        "binance" => Venue::Binance,
        other => Venue::Custom(other.to_string()),
    };
    Instrument {
        venue,
        asset_class: AssetClass::CryptoSpot,
        symbol: symbol.to_string(),
        quote_ccy: "USDT".to_string(),
        tick_size: Decimal::new(1, 8),
        lot_size: Decimal::new(1, 8),
        session: SessionCalendar::binance_24x7(),
    }
}

fn parse_timeframe(interval: &str) -> Option<Timeframe> {
    match interval.trim().to_lowercase().as_str() {
        "1m" => Some(Timeframe::M1),
        "5m" => Some(Timeframe::M5),
        "15m" => Some(Timeframe::M15),
        "30m" => Some(Timeframe::M30),
        "1h" => Some(Timeframe::H1),
        "4h" => Some(Timeframe::H4),
        "1d" => Some(Timeframe::D1),
        _ => None,
    }
}
