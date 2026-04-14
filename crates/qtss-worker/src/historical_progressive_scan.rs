//! Progressive historical detection scan — Faz 10 / P3.
//!
//! Walks each enabled symbol's bar history from the first stored bar
//! forward, calling every active detector family at each offset
//! (stepping by `scan_stride_bars`). At each offset the pivot and
//! regime engines see only the bars up to that point — so Wyckoff /
//! Elliott / Classical / Harmonic detect patterns as they would have
//! appeared in real time, not just the current end-of-history snapshot.
//!
//! Why not the live orchestrator?
//!   The live `v2_detection_orchestrator` fetches the last 3000 bars,
//!   builds a fresh pivot tree, and calls detectors ONCE against that
//!   snapshot. A multi-year range that completed 5000 bars ago never
//!   appears because it sits outside the window and falls under
//!   `max_range_age_bars`. This worker fills that gap by emitting at
//!   every historical offset where a pattern would have been visible.
//!
//! Dedup is handled by `V2DetectionRepository::list_filtered` +
//! `raw_meta.last_anchor_idx` — the same structure won't be inserted
//! twice across overlapping offsets.
//!
//! Scope choices:
//!   * Insert minimal rows: core NewDetection only. The expensive
//!     follow-up paths (projection accuracy, wyckoff structure
//!     tracker, wave_chain link) stay exclusive to the live loop —
//!     historical backfill only needs the raw detection row to exist.
//!     When the live orchestrator catches up, it will enrich the
//!     most recent detections the normal way.
//!   * Dispatch & config reuse the orchestrator's `build_runners`
//!     through pub(crate) visibility — no detector logic duplicated.
//!
//! CLAUDE.md compliance:
//!   #1 no scattered if/else: single offset loop, detector dispatch
//!      table reused from orchestrator.
//!   #2 config-driven: enable flag, tick interval, stride, minimum
//!      offset, and starting window all read from system_config.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::pivot::PivotLevel;
use qtss_pivots::{PivotConfig, PivotEngine};
use qtss_regime::{RegimeConfig, RegimeEngine};
use qtss_storage::{
    list_enabled_engine_symbols, list_recent_bars, resolve_system_u64,
    resolve_worker_enabled_flag, resolve_worker_tick_secs, DetectionFilter, EngineSymbolRow,
    NewDetection, V2DetectionRepository,
};
use rust_decimal::prelude::ToPrimitive;
use serde_json::json;
use sqlx::PgPool;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::v2_detection_orchestrator::{
    build_runners, build_instrument, parse_timeframe, split_pattern_kind,
};

pub async fn historical_progressive_scan_loop(pool: PgPool) {
    info!("historical_progressive_scan_loop started");
    let repo = Arc::new(V2DetectionRepository::new(pool.clone()));

    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "detection",
            "historical_progressive_scan.enabled",
            "QTSS_HISTORICAL_PROGRESSIVE_SCAN_ENABLED",
            false,
        )
        .await;
        let tick_secs = resolve_worker_tick_secs(
            &pool,
            "detection",
            "historical_progressive_scan.tick_secs",
            "QTSS_HISTORICAL_PROGRESSIVE_SCAN_TICK_SECS",
            3600,
            60,
        )
        .await;

        if !enabled {
            tokio::time::sleep(Duration::from_secs(tick_secs)).await;
            continue;
        }

        if let Err(e) = run_pass(&pool, repo.clone()).await {
            warn!(%e, "historical_progressive_scan pass failed");
        }

        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
    }
}

async fn run_pass(pool: &PgPool, repo: Arc<V2DetectionRepository>) -> anyhow::Result<()> {
    let stride =
        resolve_system_u64(pool, "detection", "historical_progressive_scan.stride_bars", "", 50, 1, 5000)
            .await as usize;
    let min_offset =
        resolve_system_u64(pool, "detection", "historical_progressive_scan.min_offset_bars", "", 100, 30, 5000)
            .await as usize;
    let per_symbol_cap = resolve_system_u64(
        pool,
        "detection",
        "historical_progressive_scan.max_offsets_per_pass",
        "",
        200,
        1,
        10_000,
    )
    .await as usize;

    let mode = "live"; // historical detections tagged live so charts render them alongside current

    let runners = build_runners(pool).await;
    if runners.is_empty() {
        debug!("progressive scan: no runners enabled");
        return Ok(());
    }

    let symbols = list_enabled_engine_symbols(pool).await?;
    let mut processed = 0u32;
    let mut total_offsets = 0u64;
    let mut total_inserted = 0u64;
    for sym in &symbols {
        match scan_symbol(pool, &*repo, sym, &runners, stride, min_offset, per_symbol_cap, mode).await {
            Ok((offsets, inserted)) => {
                if offsets > 0 {
                    processed += 1;
                    total_offsets += offsets as u64;
                    total_inserted += inserted as u64;
                }
            }
            Err(e) => warn!(symbol = %sym.symbol, interval = %sym.interval, %e, "progressive scan failed for symbol"),
        }
    }
    if processed > 0 {
        info!(
            processed,
            total_offsets,
            total_inserted,
            "historical_progressive_scan pass complete"
        );
    }
    Ok(())
}

async fn scan_symbol(
    pool: &PgPool,
    repo: &V2DetectionRepository,
    sym: &EngineSymbolRow,
    runners: &[Box<dyn crate::v2_detection_orchestrator::DetectorRunner>],
    stride: usize,
    min_offset: usize,
    per_symbol_cap: usize,
    mode: &'static str,
) -> anyhow::Result<(usize, usize)> {
    if !qtss_storage::is_backfill_ready(pool, sym.id).await {
        return Ok((0, 0));
    }
    let Some(timeframe) = parse_timeframe(&sym.interval) else {
        return Ok((0, 0));
    };
    let instrument = build_instrument(&sym.exchange, &sym.segment, &sym.symbol);

    // Cursor row records the last offset already scanned so we don't
    // reprocess history on every tick.
    let prior_cursor =
        get_scan_cursor(pool, &sym.exchange, &sym.segment, &sym.symbol, &sym.interval).await?;

    // Fetch everything once — a symbol with 10 years of hourly data is
    // ~87k rows, still tractable in memory and avoids re-querying per
    // offset. If it becomes a problem we can chunk.
    let mut raw = list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        200_000,
    )
    .await?;
    raw.reverse();
    if raw.len() < min_offset {
        return Ok((0, 0));
    }

    let start_offset = prior_cursor.max(min_offset);
    if start_offset >= raw.len() {
        return Ok((0, 0));
    }

    // Build engines once up to `start_offset - 1` so stepping forward
    // only costs incremental bar feeds. This is critical for keeping
    // a 87k-bar series tractable: we do one full pass per pivot /
    // regime engine, not one pass per offset.
    let mut pivot_engine = PivotEngine::new(PivotConfig::defaults())?;
    let mut regime_engine = RegimeEngine::new(RegimeConfig::defaults())?;
    let mut bars: Vec<Bar> = Vec::with_capacity(raw.len());

    // Warm-up: feed bars [0..start_offset) without scanning — we only
    // want detectors to fire at `start_offset`, `start_offset+stride`, …
    for row in &raw[..start_offset] {
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
        if pivot_engine.on_bar(&bar).is_err() {
            return Ok((0, 0));
        }
        let _ = regime_engine.on_bar(&bar);
        bars.push(bar);
    }

    let mut offsets_processed = 0usize;
    let mut inserted = 0usize;
    let mut cursor = start_offset;
    let mut latest_regime = None;

    while cursor < raw.len() && offsets_processed < per_symbol_cap {
        // Feed bars [cursor .. cursor+stride).
        let end = (cursor + stride).min(raw.len());
        for row in &raw[cursor..end] {
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
            if pivot_engine.on_bar(&bar).is_err() {
                break;
            }
            if let Ok(Some(snap)) = regime_engine.on_bar(&bar) {
                latest_regime = Some(snap);
            }
            bars.push(bar);
        }
        cursor = end;
        offsets_processed += 1;

        let Some(regime) = latest_regime.clone() else {
            continue; // regime still warming up
        };
        let tree = pivot_engine.snapshot();

        // Fire every runner against the current tree/bars. Dedupe is
        // per-detection via raw_meta.last_anchor_idx.
        for runner in runners {
            let detections = runner.detect(&tree, &bars, &instrument, timeframe, &regime);
            for detection in detections {
                let (family, subkind) = split_pattern_kind(&detection.kind);
                let last_anchor_idx = detection.anchors.last().map(|a| a.bar_index).unwrap_or(0);

                if anchor_already_seen(
                    repo,
                    &sym.exchange,
                    &sym.symbol,
                    &sym.interval,
                    family,
                    last_anchor_idx,
                )
                .await?
                {
                    continue;
                }

                // Minimal anchor enrichment: resolve bar_index → open_time
                // from the bars we just fed.
                let anchors_json = json!(detection
                    .anchors
                    .iter()
                    .map(|a| {
                        let idx = a.bar_index as usize;
                        let time = bars
                            .get(idx)
                            .map(|b| b.open_time.to_rfc3339())
                            .unwrap_or_default();
                        json!({
                            "bar_index": a.bar_index,
                            "time": time,
                            "price": a.price.to_string(),
                            "level": format!("{:?}", a.level),
                            "label": a.label,
                        })
                    })
                    .collect::<Vec<_>>());
                let regime_json = serde_json::to_value(&detection.regime_at_detection)
                    .unwrap_or_else(|_| json!({}));
                let raw_meta = json!({
                    "detection_id": detection.id,
                    "last_anchor_idx": last_anchor_idx,
                    "structural_score": detection.structural_score,
                    "source": "historical_progressive_scan",
                    "offset": cursor,
                });

                let new_row = NewDetection {
                    id: Uuid::new_v4(),
                    detected_at: Utc::now(),
                    exchange: &sym.exchange,
                    symbol: &sym.symbol,
                    timeframe: &sym.interval,
                    family,
                    subkind,
                    state: "forming",
                    structural_score: detection.structural_score,
                    invalidation_price: detection.invalidation_price,
                    anchors: anchors_json,
                    regime: regime_json,
                    raw_meta,
                    mode,
                };
                if let Err(e) = repo.insert(new_row).await {
                    warn!(symbol = %sym.symbol, family, subkind, %e, "progressive insert failed");
                    continue;
                }
                inserted += 1;
            }
        }
    }

    upsert_scan_cursor(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        cursor as i64,
        raw.len() as i64,
        inserted as i64,
    )
    .await?;

    if offsets_processed > 0 {
        info!(
            symbol = %sym.symbol,
            interval = %sym.interval,
            offsets = offsets_processed,
            inserted,
            cursor,
            total_bars = raw.len(),
            l0 = pivot_engine.snapshot().at_level(PivotLevel::L0).len(),
            l1 = pivot_engine.snapshot().at_level(PivotLevel::L1).len(),
            "progressive scan advanced"
        );
    }

    Ok((offsets_processed, inserted))
}

async fn anchor_already_seen(
    repo: &V2DetectionRepository,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
    family: &str,
    last_anchor_idx: u64,
) -> anyhow::Result<bool> {
    let rows = repo
        .list_filtered(DetectionFilter {
            exchange: Some(exchange),
            symbol: Some(symbol),
            timeframe: Some(timeframe),
            family: Some(family),
            state: None, // any state — we don't want to re-insert invalidated duplicates either
            mode: None,
            limit: 20,
        })
        .await?;
    for row in rows {
        if let Some(idx) = row.raw_meta.get("last_anchor_idx").and_then(|v| v.as_u64()) {
            if idx == last_anchor_idx {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

// ---------------------------------------------------------------------
// Cursor persistence — tracked in historical_progressive_scan_state
// (migration 0071). Raw sqlx here to avoid dragging yet another row
// type through qtss-storage for a single-purpose state row.
// ---------------------------------------------------------------------

async fn get_scan_cursor(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    timeframe: &str,
) -> anyhow::Result<usize> {
    let row: Option<(i64,)> = sqlx::query_as(
        r#"SELECT last_offset
           FROM historical_progressive_scan_state
           WHERE exchange = $1 AND segment = $2 AND symbol = $3 AND timeframe = $4"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(timeframe)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(o,)| o.max(0) as usize).unwrap_or(0))
}

async fn upsert_scan_cursor(
    pool: &PgPool,
    exchange: &str,
    segment: &str,
    symbol: &str,
    timeframe: &str,
    last_offset: i64,
    total_bars: i64,
    inserted_this_pass: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO historical_progressive_scan_state
               (exchange, segment, symbol, timeframe,
                last_offset, total_bars, total_inserted, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, now())
           ON CONFLICT (exchange, segment, symbol, timeframe) DO UPDATE SET
               last_offset     = EXCLUDED.last_offset,
               total_bars      = EXCLUDED.total_bars,
               total_inserted  = historical_progressive_scan_state.total_inserted + $7,
               updated_at      = now()"#,
    )
    .bind(exchange)
    .bind(segment)
    .bind(symbol)
    .bind(timeframe)
    .bind(last_offset)
    .bind(total_bars)
    .bind(inserted_this_pass)
    .execute(pool)
    .await?;
    Ok(())
}

// Helper — let f32 score flow through Decimal math later if needed.
#[allow(dead_code)]
fn score_to_f64(s: f32) -> f64 {
    s.to_f64().unwrap_or(0.0)
}
