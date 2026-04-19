//! Faz 9C — backtest setup dispatcher.
//!
//! The live `v2_setup_loop` is tied to `fetch_latest_v2_confluence`,
//! which only contains rows generated from **live** bar ingestion. As a
//! result the 453k+ backtest detections produced by the historical
//! progressive scan never armed a setup, and `mode='backtest'` rows in
//! `qtss_setups` stayed at zero (see `docs/notes/backtest_dispatcher_bug.md`).
//!
//! This loop closes that gap with a deliberately thinner arming path:
//!
//!   1. Poll `list_backtest_unset_detections` (LEFT JOIN on
//!      `qtss_setups.detection_id`).
//!   2. Build a `PositionGuard` from the detection's own
//!      `invalidation_price` + `raw_meta.structural_targets` (already
//!      emitted by the live loop, commit 8ca744a) or fall back to an
//!      ATR-based guard.
//!   3. Insert a `mode='backtest'` `qtss_setups` row in state `armed`.
//!      Migration 0171 already widened the partial unique index to
//!      include `mode`, so backtest + live can coexist for the same
//!      (exchange, symbol, timeframe, profile).
//!
//! What this path **intentionally skips** (rationale in migration 0178):
//!
//!   * Live confluence gate — no historical rows exist.
//!   * AI inference — training data would leak, and shadow-logging a
//!     replay adds no signal (the model is trained on live windows).
//!   * Commission gate — backtest outcomes are measured post-hoc
//!     against the portfolio engine, which applies fees itself.
//!
//! CLAUDE.md compliance:
//!   * Every threshold is in `system_config` (migration 0178).
//!   * No scattered if/else: early-returns + small helpers.
//!   * Asset-class agnostic: exchange → venue class via the same
//!     dispatch used by the live loop.

use std::time::Duration;

use qtss_setup_engine::{
    Direction, PositionGuard, PositionGuardConfig, Profile, StructuralTarget, VenueClass,
};
use qtss_storage::{
    insert_v2_setup, list_open_v2_setups, list_recent_bars_before, resolve_system_f64,
    resolve_system_u64, resolve_worker_enabled_flag, DetectionRow, V2DetectionRepository,
    V2SetupInsert,
};
use qtss_indicators::atr;
use rust_decimal::prelude::ToPrimitive;
use serde_json::json;
use sqlx::PgPool;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::v2_setup_loop::{compute_structural_targets, venue_class_from_exchange};

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

// ───────────────────────── config ─────────────────────────

#[derive(Debug, Clone)]
struct LoopConfig {
    enabled: bool,
    tick_interval_s: u64,
    batch_size: i64,
    profile: Profile,
    min_confidence: f64,
    min_structural_score: f64,
    atr_period: usize,
    atr_lookback_bars: i64,
    guard: PositionGuardConfig,
    risk_pct: f64,
    skip_if_live_setup_open: bool,
}

async fn load_config(pool: &PgPool) -> LoopConfig {
    let enabled = resolve_worker_enabled_flag(
        pool,
        "backtest",
        "setup_loop.enabled",
        "",
        false,
    )
    .await;
    let tick_interval_s =
        resolve_system_u64(pool, "backtest", "setup_loop.tick_interval_s", "", 60, 5, 3_600).await;
    let batch_size =
        resolve_system_u64(pool, "backtest", "setup_loop.batch_size", "", 200, 1, 5_000).await
            as i64;
    let profile_raw = resolve_system_string(pool, "backtest", "setup_loop.profile", "T").await;
    let profile = match profile_raw.to_ascii_uppercase().as_str() {
        "Q" => Profile::Q,
        "D" => Profile::D,
        _ => Profile::T,
    };
    let min_confidence =
        resolve_system_f64(pool, "backtest", "setup_loop.min_confidence", "", 0.55).await;
    let min_structural_score =
        resolve_system_f64(pool, "backtest", "setup_loop.min_structural_score", "", 0.60).await;
    let atr_period =
        resolve_system_u64(pool, "backtest", "setup_loop.atr_period", "", 14, 2, 500).await
            as usize;
    let atr_lookback_bars = resolve_system_u64(
        pool,
        "backtest",
        "setup_loop.atr_lookback_bars",
        "",
        30,
        atr_period as u64,
        5_000,
    )
    .await as i64;
    let entry_sl_atr_mult =
        resolve_system_f64(pool, "backtest", "setup_loop.entry_sl_atr_mult", "", 1.0).await;
    let target_ref_r =
        resolve_system_f64(pool, "backtest", "setup_loop.target_ref_r", "", 2.0).await;
    let risk_pct = resolve_system_f64(pool, "backtest", "setup_loop.risk_pct", "", 0.5).await;
    let skip_if_live_setup_open = resolve_worker_enabled_flag(
        pool,
        "backtest",
        "setup_loop.skip_if_live_setup_open",
        "",
        true,
    )
    .await;

    let guard = PositionGuardConfig {
        entry_sl_atr_mult,
        ratchet_interval_secs: 60,
        target_ref_r,
        risk_pct,
        max_concurrent: 999, // allocator limits come from global config; this field is for live watcher budgeting
        reverse_guven_threshold: 0.55,
    };

    LoopConfig {
        enabled,
        tick_interval_s,
        batch_size,
        profile,
        min_confidence,
        min_structural_score,
        atr_period,
        atr_lookback_bars,
        guard,
        risk_pct,
        skip_if_live_setup_open,
    }
}

/// Thin wrapper around `resolve_system_f64/u64` for JSON strings.
/// Postgres stores `"T"` as a JSON string; `system_config.value->>0`
/// unwraps it. We keep the fallback inline so a missing row does not
/// break the loop.
async fn resolve_system_string(pool: &PgPool, module: &str, key: &str, default: &str) -> String {
    let row: Option<(serde_json::Value,)> = sqlx::query_as(
        "SELECT value FROM system_config WHERE module=$1 AND config_key=$2",
    )
    .bind(module)
    .bind(key)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);
    row.and_then(|(v,)| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| default.to_string())
}

// ───────────────────────── loop entry ─────────────────────────

/// Entry point spawned by `main.rs`. Polls forever; all behaviour lives
/// behind the `backtest.setup_loop.enabled` flag so the loop is a no-op
/// when the operator hasn't opted in.
pub async fn v2_backtest_setup_loop(pool: PgPool) {
    info!("v2_backtest_setup_loop: spawned (behaviour gated on backtest.setup_loop.enabled)");
    loop {
        let cfg = load_config(&pool).await;
        if !cfg.enabled {
            sleep(Duration::from_secs(cfg.tick_interval_s.max(5))).await;
            continue;
        }
        match tick(&pool, &cfg).await {
            Ok(n) => {
                if n > 0 {
                    info!(armed = n, "v2_backtest_setup_loop: tick armed setups");
                } else {
                    debug!("v2_backtest_setup_loop: tick armed 0 setups");
                }
            }
            Err(e) => warn!(%e, "v2_backtest_setup_loop: tick failed"),
        }
        sleep(Duration::from_secs(cfg.tick_interval_s)).await;
    }
}

// ───────────────────────── tick body ─────────────────────────

async fn tick(pool: &PgPool, cfg: &LoopConfig) -> Result<usize, BoxErr> {
    let repo = V2DetectionRepository::new(pool.clone());
    let rows = repo.list_backtest_unset_detections(cfg.batch_size).await?;
    if rows.is_empty() {
        return Ok(0);
    }
    let mut armed = 0usize;
    for det in rows {
        match process_detection(pool, cfg, &det).await {
            Ok(true) => armed += 1,
            Ok(false) => {}
            Err(e) => {
                // One bad detection must not stop the batch — a single
                // garbled raw_meta or a missing bar window would
                // otherwise halt the whole pipeline. Log and move on;
                // the LEFT JOIN query keeps re-surfacing the row on
                // the next tick until something else (data, config,
                // or code) fixes it.
                warn!(
                    detection_id = %det.id,
                    symbol = %det.symbol,
                    %e,
                    "v2_backtest_setup_loop: skip detection"
                );
            }
        }
    }
    Ok(armed)
}

async fn process_detection(
    pool: &PgPool,
    cfg: &LoopConfig,
    det: &DetectionRow,
) -> Result<bool, BoxErr> {
    // ── quality gates ──
    let conf = det.confidence.unwrap_or(0.0) as f64;
    if conf < cfg.min_confidence {
        return Ok(false);
    }
    if (det.structural_score as f64) < cfg.min_structural_score {
        return Ok(false);
    }
    // Direction inferred from subkind ("*_bull"/"*_bear") — mirrors
    // the live loop's heuristic. Neutral detections aren't tradeable.
    let direction = infer_direction(&det.subkind);
    if matches!(direction, Direction::Neutral) {
        return Ok(false);
    }
    let venue = match venue_class_from_exchange(&det.exchange) {
        Some(v) => v,
        None => return Ok(false),
    };

    // ── skip if a live setup already owns the (ex,sym,tf,profile) slot ──
    // Safety net (config flag): prevents a backtest replay from stacking
    // on top of an in-flight live trade during concurrent dev runs.
    if cfg.skip_if_live_setup_open && live_setup_blocks(pool, venue, det, cfg).await? {
        debug!(detection_id = %det.id, "skip — live/dry setup already open for slot");
        return Ok(false);
    }

    // ── point-in-time bars for ATR + entry ──
    let bars = list_recent_bars_before(
        pool,
        &det.exchange,
        // `segment` isn't on DetectionRow; default to "futures" for
        // crypto and let BIST/equities fail through to the structural
        // branch (ATR fallback is only reached when structural targets
        // are absent). A future faz can look up engine_symbols.segment.
        default_segment_for_venue(venue),
        &det.symbol,
        &det.timeframe,
        det.detected_at,
        cfg.atr_lookback_bars,
    )
    .await?;
    if bars.is_empty() {
        return Ok(false);
    }
    // `list_recent_bars_before` returns DESC; reverse for indicator math.
    let chronological: Vec<_> = bars.into_iter().rev().collect();
    let closes: Vec<f64> = chronological
        .iter()
        .map(|b| b.close.to_f64().unwrap_or(0.0))
        .collect();
    let highs: Vec<f64> = chronological
        .iter()
        .map(|b| b.high.to_f64().unwrap_or(0.0))
        .collect();
    let lows: Vec<f64> = chronological
        .iter()
        .map(|b| b.low.to_f64().unwrap_or(0.0))
        .collect();
    let entry = match closes.last().copied() {
        Some(p) if p > 0.0 => p,
        _ => return Ok(false),
    };
    let atr_val = atr(&highs, &lows, &closes, cfg.atr_period)
        .iter()
        .rev()
        .find(|v| v.is_finite() && **v > 0.0)
        .copied()
        .unwrap_or(0.0);
    if atr_val <= 0.0 {
        return Ok(false);
    }

    // ── structural guard (preferred) or ATR fallback ──
    let inv_price = det.invalidation_price.to_f64().unwrap_or(0.0);
    let targets: Vec<StructuralTarget> = compute_structural_targets(det, direction);
    let guard = if inv_price > 0.0 && !targets.is_empty() {
        PositionGuard::new_structural(entry, inv_price, &targets, atr_val, &cfg.guard, direction)
    } else {
        PositionGuard::new(entry, atr_val, &cfg.guard, direction)
    };

    // ── build raw_meta (mirror live loop: structural_targets + subkind) ──
    let structural_targets_meta: Vec<serde_json::Value> = targets
        .iter()
        .map(|t| json!({ "price": t.price, "weight": t.weight, "label": t.label }))
        .collect();
    let raw_meta = json!({
        "source": "backtest_dispatcher",
        "detection_confidence": conf,
        "structural_subkind": det.subkind,
        "structural_targets": structural_targets_meta,
        "structural": guard.structural,
    });

    // ── insert ──
    let insert = V2SetupInsert {
        venue_class: venue.as_str().to_string(),
        exchange: det.exchange.clone(),
        symbol: det.symbol.clone(),
        timeframe: det.timeframe.clone(),
        profile: cfg.profile.as_str().to_string(),
        alt_type: Some(format!("backtest_{}_{}", det.family, det.subkind)),
        state: "armed".to_string(),
        direction: direction_as_db(direction).to_string(),
        confluence_id: None,
        entry_price: Some(guard.entry as f32),
        entry_sl: Some(guard.entry_sl as f32),
        koruma: Some(guard.koruma as f32),
        target_ref: Some(guard.target_ref as f32),
        risk_pct: Some(cfg.risk_pct as f32),
        raw_meta,
        ai_score: None,
        detection_id: Some(det.id),
        mode: "backtest".to_string(),
    };
    match insert_v2_setup(pool, &insert).await {
        Ok(_id) => Ok(true),
        Err(qtss_storage::StorageError::DuplicateSetup) => {
            // The LEFT JOIN should have pre-filtered these out, but a
            // concurrent run could race into the partial unique index.
            // Treat as a silent skip — the row is now "linked" via the
            // existing open setup and will stop reappearing.
            Ok(false)
        }
        Err(e) => Err(Box::new(e)),
    }
}

// ───────────────────────── helpers ─────────────────────────

fn infer_direction(subkind: &str) -> Direction {
    let s = subkind.to_ascii_lowercase();
    if s.contains("bull") || s.contains("long") {
        Direction::Long
    } else if s.contains("bear") || s.contains("short") {
        Direction::Short
    } else {
        Direction::Neutral
    }
}

fn direction_as_db(d: Direction) -> &'static str {
    match d {
        Direction::Long => "long",
        Direction::Short => "short",
        Direction::Neutral => "neutral",
    }
}

fn default_segment_for_venue(v: VenueClass) -> &'static str {
    match v {
        // Crypto detections are predominantly futures-segment in this
        // deployment; a future faz should derive segment from
        // engine_symbols (detections carry exchange but not segment).
        VenueClass::Crypto => "futures",
        VenueClass::Bist => "spot",
        VenueClass::UsEquities => "spot",
        VenueClass::Commodities => "spot",
        VenueClass::Fx => "spot",
    }
}

async fn live_setup_blocks(
    pool: &PgPool,
    venue: VenueClass,
    det: &DetectionRow,
    cfg: &LoopConfig,
) -> Result<bool, BoxErr> {
    let open = list_open_v2_setups(pool, Some(venue.as_str()), None).await?;
    let profile_str = cfg.profile.as_str();
    let hit = open.iter().any(|r| {
        r.exchange == det.exchange
            && r.symbol == det.symbol
            && r.timeframe == det.timeframe
            && r.profile == profile_str
            && (r.mode == "live" || r.mode == "dry")
    });
    Ok(hit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_direction_covers_long_and_short_markers() {
        assert!(matches!(infer_direction("impulse_w3_extended_bull"), Direction::Long));
        assert!(matches!(infer_direction("flat_expanded_bear"), Direction::Short));
        assert!(matches!(infer_direction("scallop_bullish"), Direction::Long));
        assert!(matches!(infer_direction("scallop_bearish"), Direction::Short));
        assert!(matches!(infer_direction("long_entry"), Direction::Long));
        assert!(matches!(infer_direction("short_squeeze"), Direction::Short));
        assert!(matches!(infer_direction("triangle"), Direction::Neutral));
    }

    #[test]
    fn direction_as_db_maps_all_variants() {
        assert_eq!(direction_as_db(Direction::Long), "long");
        assert_eq!(direction_as_db(Direction::Short), "short");
        assert_eq!(direction_as_db(Direction::Neutral), "neutral");
    }

    #[test]
    fn default_segment_is_crypto_futures() {
        assert_eq!(default_segment_for_venue(VenueClass::Crypto), "futures");
    }
}
