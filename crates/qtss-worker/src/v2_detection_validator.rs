//! v2 detection validator loop — Faz 7 Adım 3.
//!
//! Drains forming detections out of `qtss_v2_detections` (rows where
//! `confidence IS NULL`), reconstructs the in-memory `Detection` from
//! the persisted JSON columns, runs the `qtss-validator` confirmation
//! channels and writes back either:
//!
//! * `mark_validated(confidence, channel_scores)` when the blended
//!   confidence clears `detection.validator.min_confidence`, or
//! * `update_state("invalidated")` when it does not — the row stays in
//!   the table for audit but disappears from the live overlay.
//!
//! Why a separate loop instead of inlining inside the orchestrator:
//! the orchestrator stays a thin "what do I see right now" producer.
//! The validator is a consumer that can run at its own cadence and be
//! disabled independently. CLAUDE.md #3 (detector / strategy / adapter
//! separation): detectors do not know about validators.
//!
//! Channels are wired via the trait dispatch already exposed by
//! `qtss-validator` (CLAUDE.md #1 — no scattered if/else). All toggles,
//! batch sizes and weights come from `system_config` (CLAUDE.md #2).

use std::sync::Arc;
use std::time::Duration;

use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::detection::{
    Detection, PatternKind, PatternState, PivotRef, ValidatedDetection,
};
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_eventbus::{topics::PATTERN_VALIDATED, EventBus, InProcessBus};
use qtss_storage::{
    list_recent_bars, resolve_system_f64, resolve_system_u64, resolve_worker_enabled_flag,
    DetectionFilter, DetectionOutcomeRepository, DetectionRow, V2DetectionRepository,
};
use qtss_validator::{
    is_higher_timeframe, BreakoutBodyAtr, BreakoutCloseQuality, HistoricalHitRate, HitRateStat,
    MultiTfRegimeConfluence, MultiTfRegimeContext, MultiTimeframeConfluence, RegimeAlignment,
    RetestQuality, ValidationContext, Validator, ValidatorConfig, VolumeConfirmation,
};
use std::collections::HashMap;
use serde_json::json;
use sqlx::PgPool;
use tracing::{debug, info, warn};

use crate::v2_detection_orchestrator::{build_instrument, parse_timeframe};

pub async fn v2_detection_validator_loop(pool: PgPool, bus: Arc<InProcessBus>) {
    info!("v2 detection validator loop spawned (gated on detection.validator.enabled)");
    let repo = Arc::new(V2DetectionRepository::new(pool.clone()));

    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "detection",
            "validator.enabled",
            "QTSS_DETECTION_VALIDATOR_ENABLED",
            false,
        )
        .await;

        let tick_secs = resolve_system_u64(
            &pool,
            "detection",
            "validator.tick_interval_s",
            "QTSS_DETECTION_VALIDATOR_TICK_S",
            5,
            1,
            3600,
        )
        .await;

        if !enabled {
            tokio::time::sleep(Duration::from_secs(tick_secs)).await;
            continue;
        }

        match run_pass(&pool, repo.clone(), bus.clone()).await {
            Ok(stats) => {
                if stats.scanned > 0 {
                    info!(
                        scanned = stats.scanned,
                        validated = stats.validated,
                        invalidated = stats.invalidated,
                        skipped = stats.skipped,
                        "v2 detection validator pass complete"
                    );
                } else {
                    debug!("v2 detection validator pass: no pending rows");
                }
            }
            Err(e) => warn!(%e, "v2 detection validator pass failed"),
        }

        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
    }
}

#[derive(Default)]
struct PassStats {
    scanned: usize,
    validated: usize,
    invalidated: usize,
    skipped: usize,
}

async fn run_pass(
    pool: &PgPool,
    repo: Arc<V2DetectionRepository>,
    bus: Arc<InProcessBus>,
) -> anyhow::Result<PassStats> {
    let mut stats = PassStats::default();

    let batch_limit = resolve_system_u64(
        pool,
        "detection",
        "validator.batch_limit",
        "QTSS_DETECTION_VALIDATOR_BATCH_LIMIT",
        50,
        1,
        1000,
    )
    .await as i64;

    let validator = build_validator(pool).await?;
    let hit_rates = load_hit_rates(repo.as_ref(), pool).await;
    let tbm_boost_cfg = TbmBoostConfig::load(pool).await;

    let htf_lookup_limit = resolve_system_u64(
        pool,
        "detection",
        "validator.htf_lookup_limit",
        "QTSS_DETECTION_VALIDATOR_HTF_LOOKUP_LIMIT",
        20,
        1,
        500,
    )
    .await as i64;

    let rows = repo.list_pending_validation(batch_limit).await?;
    for row in rows {
        stats.scanned += 1;
        let id = row.id;
        let Some(detection) = reconstruct_detection(&row) else {
            stats.skipped += 1;
            debug!(%id, "validator skip: could not reconstruct detection");
            continue;
        };
        let higher_tf_detections =
            load_higher_tf_detections(repo.as_ref(), &row, &detection, htf_lookup_limit).await;
        let multi_tf_regime = load_multi_tf_regime(pool, &row.symbol).await;
        // P2 — load recent bars only for families that actually consume
        // them (currently classical breakout/volume channels). Other
        // families' channels just return `None` without bar data, so we
        // save the roundtrip.
        let recent_bars = if row.family == "classical" {
            load_recent_bars_for_validation(pool, &row).await
        } else {
            Vec::new()
        };
        let ctx = ValidationContext {
            higher_tf_detections,
            hit_rates: hit_rates.clone(),
            multi_tf_regime,
            recent_bars,
        };
        match validator.validate(detection, &ctx) {
            Some(mut v) => {
                if tbm_boost_cfg.enabled {
                    if let Some(delta) =
                        tbm_boost_for(repo.as_ref(), &row, &tbm_boost_cfg).await
                    {
                        v.confidence = (v.confidence + delta).clamp(0.0, 1.0);
                    }
                }
                let scores_json = serde_json::to_value(&v.channel_scores).unwrap_or_else(|_| json!([]));
                repo.mark_validated(id, v.confidence, scores_json, v.validated_at)
                    .await?;
                publish_validated(&bus, &v).await;
                stats.validated += 1;
            }
            None => {
                repo.update_state(id, "invalidated").await?;
                stats.invalidated += 1;
            }
        }
    }

    Ok(stats)
}

/// Build the per-pass hit-rate map. The key shape mirrors
/// `qtss_validator::pattern_key()` exactly: `"<family>:<subkind>@<TF>"`
/// where `<TF>` is the `Timeframe` Debug variant ("M1", "H4", …).
/// Failures are logged and degrade gracefully to an empty map — the
/// historical channel will simply abstain from voting.
async fn load_hit_rates(repo: &V2DetectionRepository, pool: &PgPool) -> HashMap<String, HitRateStat> {
    let mut out: HashMap<String, HitRateStat> = HashMap::new();

    // Prefer real outcomes from detection_outcomes table (self-learning).
    let outcome_repo = DetectionOutcomeRepository::new(pool.clone());
    if let Ok(real_rates) = outcome_repo.hit_rates().await {
        for r in &real_rates {
            if r.total < 5 {
                continue; // Need minimum sample size.
            }
            let Some(tf_dbg) = timeframe_debug_label(&r.timeframe) else {
                continue;
            };
            let key = format!("{}:{}@{}", r.family, r.subkind, tf_dbg);
            out.insert(
                key,
                HitRateStat {
                    samples: r.total as u32,
                    hit_rate: r.win_rate as f32,
                },
            );
        }
    }

    // Fall back to the cheap proxy for patterns that have no real outcomes yet.
    let rows = match repo.historical_outcome_counts().await {
        Ok(rows) => rows,
        Err(e) => {
            warn!(%e, "v2 detection validator: hit_rate query failed");
            return out;
        }
    };
    for row in rows {
        let Some(tf_dbg) = timeframe_debug_label(&row.timeframe) else {
            continue;
        };
        let key = format!("{}:{}@{}", row.family, row.subkind, tf_dbg);
        // Only insert if real outcomes didn't already provide this key.
        if out.contains_key(&key) {
            continue;
        }
        let total = row.validated_count + row.invalidated_count;
        if total <= 0 {
            continue;
        }
        let hit_rate = row.validated_count as f32 / total as f32;
        out.insert(
            key,
            HitRateStat {
                samples: total as u32,
                hit_rate,
            },
        );
    }
    out
}

/// HTF confluence feed — Faz 7 Adım 9. For each pending row, load the
/// most recent already-validated detections on *other* timeframes for
/// the same `(exchange, symbol)`, reconstruct them, and keep only the
/// strictly-higher TFs. The `MultiTimeframeConfluence` channel filters
/// further by family/direction; this loop just makes sure it has
/// something to look at. Failures degrade gracefully to an empty list
/// — the channel will simply abstain.
/// Load the trailing bar window used by the P2 classical validation
/// channels (breakout close, body/ATR, volume). Returns oldest..newest.
/// Failure or missing data degrades to an empty vec — the channels
/// simply abstain.
async fn load_recent_bars_for_validation(pool: &PgPool, row: &DetectionRow) -> Vec<Bar> {
    // 60 bars covers a 14-period ATR + enough pattern history for the
    // volume-contraction check without being wasteful.
    const LIMIT: i64 = 60;
    let segment = "futures"; // same default as the orchestrator birth path
    let rows = match list_recent_bars(pool, &row.exchange, segment, &row.symbol, &row.timeframe, LIMIT)
        .await
    {
        Ok(rs) => rs,
        Err(e) => {
            debug!(%e, symbol = %row.symbol, tf = %row.timeframe, "load_recent_bars failed");
            return Vec::new();
        }
    };
    let tf = match parse_timeframe(&row.timeframe) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let instrument = build_instrument(&row.exchange, segment, &row.symbol);
    // market_bars is returned newest-first; reverse so channels get
    // oldest..newest (matches Bar ordering conventions).
    let mut bars: Vec<Bar> = rows
        .into_iter()
        .rev()
        .map(|r| Bar {
            instrument: instrument.clone(),
            timeframe: tf,
            open_time: r.open_time,
            open: r.open,
            high: r.high,
            low: r.low,
            close: r.close,
            volume: r.volume,
            closed: true,
        })
        .collect();
    // Guard: drop any future-stamped bars (shouldn't happen, defensive).
    let now = chrono::Utc::now();
    bars.retain(|b| b.open_time <= now);
    bars
}

async fn load_higher_tf_detections(
    repo: &V2DetectionRepository,
    row: &DetectionRow,
    detection: &Detection,
    limit: i64,
) -> Vec<Detection> {
    let rows = match repo
        .list_recent_for_symbol_htf(&row.exchange, &row.symbol, &row.timeframe, limit)
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            warn!(%e, "v2 detection validator: htf lookup failed");
            return Vec::new();
        }
    };
    let base_tf = detection.timeframe;
    rows.into_iter()
        .filter_map(|r| {
            let d = reconstruct_detection(&r)?;
            is_higher_timeframe(d.timeframe, base_tf).then_some(d)
        })
        .collect()
}

/// Mirror of `parse_timeframe` but going the other way: from the
/// engine_symbols string ("1h"/"4h"/...) to the `Timeframe` enum's
/// Debug variant name. Kept here so it stays adjacent to the call site
/// — the validator is the only consumer.
fn timeframe_debug_label(interval: &str) -> Option<&'static str> {
    let s = interval.trim().to_lowercase();
    let label = match s.as_str() {
        "1m" => "M1",
        "3m" => "M3",
        "5m" => "M5",
        "15m" => "M15",
        "30m" => "M30",
        "1h" => "H1",
        "2h" => "H2",
        "4h" => "H4",
        "6h" => "H6",
        "8h" => "H8",
        "12h" => "H12",
        "1d" => "D1",
        "3d" => "D3",
        "1w" => "W1",
        "1mo" | "1mn" => "Mn1",
        _ => return None,
    };
    Some(label)
}

/// Fan a successful validation out to the in-process event bus on the
/// canonical [`PATTERN_VALIDATED`] topic. Strategy providers (Faz 7
/// Adım 6 — strategy bağlantısı) subscribe here to consume the
/// `ValidatedDetection` envelope. A publish failure is non-fatal — the
/// row is already persisted, the bus is best-effort delivery.
async fn publish_validated(bus: &InProcessBus, v: &ValidatedDetection) {
    if let Err(e) = bus.publish(PATTERN_VALIDATED, v).await {
        warn!(%e, "failed to publish pattern.validated event");
    }
}

/// Build the validator from `system_config`. The default channel set
/// matches the three reference impls in `qtss-validator`. Adding a new
/// channel means writing one impl + one `register` line — no central
/// match arm to edit (CLAUDE.md #1).
async fn build_validator(pool: &PgPool) -> anyhow::Result<Validator> {
    let min_conf = resolve_system_f64(
        pool,
        "detection",
        "validator.min_confidence",
        "QTSS_DETECTION_VALIDATOR_MIN_CONFIDENCE",
        0.55,
    )
    .await as f32;
    let structural_weight = resolve_system_f64(
        pool,
        "detection",
        "validator.structural_weight",
        "QTSS_DETECTION_VALIDATOR_STRUCTURAL_WEIGHT",
        0.5,
    )
    .await as f32;
    let hit_rate_min_samples = resolve_system_u64(
        pool,
        "detection",
        "validator.hit_rate_min_samples",
        "QTSS_DETECTION_VALIDATOR_HIT_RATE_MIN_SAMPLES",
        20,
        1,
        100_000,
    )
    .await as u32;

    // P2 — classical breakout / volume channels (config-driven knobs).
    let atr_period = resolve_system_u64(
        pool, "detection", "validator.classical.atr_period",
        "QTSS_DETECTION_VALIDATOR_ATR_PERIOD", 14, 2, 200,
    )
    .await as usize;
    let min_body_atr = resolve_system_f64(
        pool, "detection", "validator.classical.min_body_atr_mult",
        "QTSS_DETECTION_VALIDATOR_MIN_BODY_ATR", 1.0,
    )
    .await;
    let max_body_atr = resolve_system_f64(
        pool, "detection", "validator.classical.max_body_atr_mult",
        "QTSS_DETECTION_VALIDATOR_MAX_BODY_ATR", 3.0,
    )
    .await;
    let min_bo_vol = resolve_system_f64(
        pool, "detection", "validator.classical.min_breakout_vol_mult",
        "QTSS_DETECTION_VALIDATOR_MIN_BREAKOUT_VOL", 1.5,
    )
    .await;
    let max_late_early = resolve_system_f64(
        pool, "detection", "validator.classical.max_late_to_early_vol_ratio",
        "QTSS_DETECTION_VALIDATOR_MAX_LATE_EARLY_VOL", 1.0,
    )
    .await;
    // P6 — retest channel.
    let retest_tol = resolve_system_f64(
        pool, "detection", "validator.classical.retest_tolerance_pct",
        "QTSS_DETECTION_VALIDATOR_RETEST_TOL", 0.005,
    )
    .await;
    let retest_max_bars = resolve_system_u64(
        pool, "detection", "validator.classical.retest_max_bars_after_breakout",
        "QTSS_DETECTION_VALIDATOR_RETEST_MAX_BARS", 30, 1, 500,
    )
    .await as usize;

    let mut cfg = ValidatorConfig::defaults();
    cfg.min_confidence = min_conf.clamp(0.0, 1.0);
    cfg.structural_weight = structural_weight.clamp(0.0, 1.0);
    // Default weights for the new channels; operators can override via
    // system_config per key `detection.validator.weight.<channel_name>`
    // (wiring planned in a follow-up patch).
    cfg.channel_weights.push(("breakout_close_quality".into(), 1.0));
    cfg.channel_weights.push(("breakout_body_atr".into(), 0.75));
    cfg.channel_weights.push(("volume_confirmation".into(), 1.0));
    // P6 — retest is a strong continuation confirmation; same weight as
    // the breakout-close channel so a clean retest meaningfully bumps
    // confidence on top of the initial break score.
    cfg.channel_weights.push(("retest_quality".into(), 1.0));

    let mut validator = Validator::new(cfg)
        .map_err(|e| anyhow::anyhow!("validator config invalid: {e}"))?;
    validator.register(Arc::new(RegimeAlignment));
    validator.register(Arc::new(MultiTimeframeConfluence));
    validator.register(Arc::new(MultiTfRegimeConfluence));
    validator.register(Arc::new(HistoricalHitRate {
        min_samples: hit_rate_min_samples,
    }));
    validator.register(Arc::new(BreakoutCloseQuality));
    validator.register(Arc::new(BreakoutBodyAtr {
        atr_period,
        min_body_atr_mult: min_body_atr.max(0.1),
        max_body_atr_mult: max_body_atr.max(min_body_atr + 0.1),
    }));
    validator.register(Arc::new(VolumeConfirmation {
        min_breakout_vol_mult: min_bo_vol.max(1.0),
        max_late_to_early_ratio: max_late_early.max(0.1),
    }));
    validator.register(Arc::new(RetestQuality {
        tolerance_pct: retest_tol.clamp(0.0, 0.1),
        max_bars_after_breakout: retest_max_bars.max(1),
    }));
    Ok(validator)
}

/// Best-effort reverse of the orchestrator's persistence step. The
/// JSON columns were written from the same domain types so they
/// round-trip cleanly; missing pieces (segment, ValidationContext
/// extras) are filled with neutral defaults.
fn reconstruct_detection(row: &DetectionRow) -> Option<Detection> {
    let Some(timeframe) = parse_timeframe(&row.timeframe) else {
        warn!(id = %row.id, tf = %row.timeframe, "reconstruct: unknown timeframe");
        return None;
    };
    // We don't keep `segment` on the row — fall back to the default
    // crypto-spot/futures heuristic by passing empty segment, which the
    // orchestrator's parser already handles.
    let instrument = build_instrument(&row.exchange, "", &row.symbol);
    let kind = build_pattern_kind(&row.family, &row.subkind);
    let anchors: Vec<PivotRef> = serde_json::from_value(row.anchors.clone()).unwrap_or_default();
    let regime: RegimeSnapshot = serde_json::from_value(row.regime.clone())
        .unwrap_or_else(|_| RegimeSnapshot::neutral_default());
    let state = match row.state.as_str() {
        "forming" => PatternState::Forming,
        "confirmed" => PatternState::Confirmed,
        "completed" => PatternState::Completed,
        _ => PatternState::Forming,
    };
    Some(Detection {
        id: row.id,
        instrument,
        timeframe,
        kind,
        state,
        anchors,
        structural_score: row.structural_score,
        invalidation_price: row.invalidation_price,
        regime_at_detection: regime,
        detected_at: row.detected_at,
        raw_meta: row.raw_meta.clone(),
        // Validator rehydrates Detection from a stored row — projection
        // and sub-wave decomposition live in raw_meta JSON, not as
        // typed fields here. The validator never re-emits these so
        // empty vecs are the correct round-trip default.
        projected_anchors: Vec::new(),
        sub_wave_anchors: Vec::new(),
    })
}

// ---------------------------------------------------------------------
// TBM confluence boost — Faz 7.5 Adım 4
// ---------------------------------------------------------------------
//
// When the TBM detector has emitted a fresh `bottom_setup` / `top_setup`
// for the same `(exchange, symbol, timeframe)` and the candidate
// detection's bias agrees, we lift the validator's confidence by a
// configurable delta. This stays *outside* `qtss-validator` on purpose:
// the boost is a worker-side post-process so the detector/validator
// crates remain unaware of TBM and CLAUDE.md #3 layering holds.

struct TbmBoostConfig {
    enabled: bool,
    max_delta: f32,
}

impl TbmBoostConfig {
    async fn load(pool: &PgPool) -> Self {
        let enabled = resolve_worker_enabled_flag(
            pool,
            "validator",
            "tbm_boost.enabled",
            "QTSS_VALIDATOR_TBM_BOOST_ENABLED",
            false,
        )
        .await;
        let max_delta = resolve_system_f64(
            pool,
            "validator",
            "tbm_boost.max_delta",
            "QTSS_VALIDATOR_TBM_BOOST_MAX_DELTA",
            0.15,
        )
        .await as f32;
        Self {
            enabled,
            max_delta: max_delta.clamp(0.0, 1.0),
        }
    }
}

/// Returns the additive confidence delta TBM contributes to the row,
/// or `None` when there's no agreeing TBM setup. Direction agreement is
/// inferred from the candidate `subkind`: any `*_bull*`/`*_bottom*`
/// pattern pairs with TBM `bottom_setup`, mirror for bears. Magnitude
/// scales with the persisted TBM score (0..100 → 0..max_delta).
async fn tbm_boost_for(
    repo: &V2DetectionRepository,
    row: &DetectionRow,
    cfg: &TbmBoostConfig,
) -> Option<f32> {
    let bias = subkind_bias(&row.subkind)?;
    let target_subkind = match bias {
        Bias::Bullish => "bottom_setup",
        Bias::Bearish => "top_setup",
    };

    let candidates = repo
        .list_filtered(DetectionFilter {
            exchange: Some(&row.exchange),
            symbol: Some(&row.symbol),
            timeframe: Some(&row.timeframe),
            family: Some("tbm"),
            state: None,
            mode: None,
            limit: 5,
        })
        .await
        .ok()?;
    let tbm_row = candidates
        .into_iter()
        .find(|r| r.subkind == target_subkind)?;

    let score = tbm_row
        .raw_meta
        .get("tbm_score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    if score <= 0.0 {
        return None;
    }
    let normalized = (score / 100.0).clamp(0.0, 1.0) as f32;
    Some(cfg.max_delta * normalized)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bias {
    Bullish,
    Bearish,
}

fn subkind_bias(subkind: &str) -> Option<Bias> {
    let s = subkind.to_ascii_lowercase();
    if s.contains("bull") || s.contains("bottom") || s.contains("inverse_head") {
        Some(Bias::Bullish)
    } else if s.contains("bear") || s.contains("top") || s.contains("head_and_shoulders") {
        Some(Bias::Bearish)
    } else {
        None
    }
}

fn build_pattern_kind(family: &str, subkind: &str) -> PatternKind {
    let sub = subkind.to_string();
    match family {
        "elliott" => PatternKind::Elliott(sub),
        "harmonic" => PatternKind::Harmonic(sub),
        "classical" => PatternKind::Classical(sub),
        "wyckoff" => PatternKind::Wyckoff(sub),
        "range" => PatternKind::Range(sub),
        _ => PatternKind::Custom(sub),
    }
}

/// Load multi-TF regime confluence from the regime_snapshots table.
async fn load_multi_tf_regime(
    pool: &sqlx::PgPool,
    symbol: &str,
) -> Option<MultiTfRegimeContext> {
    use qtss_domain::v2::regime::RegimeKind;

    let rows = qtss_storage::latest_snapshots_for_symbol(pool, symbol)
        .await
        .ok()?;
    if rows.is_empty() {
        return None;
    }

    let tf_weights_str = qtss_storage::resolve_system_string(
        pool, "regime", "tf_weights", "",
        r#"{"5m":0.1,"15m":0.15,"1h":0.25,"4h":0.30,"1d":0.20}"#,
    ).await;
    let tf_weights: std::collections::HashMap<String, f64> =
        serde_json::from_str(&tf_weights_str)
            .unwrap_or_else(|_| qtss_regime::multi_tf::default_tf_weights());

    let snap_pairs: Vec<(String, qtss_domain::v2::regime::RegimeSnapshot)> = rows
        .iter()
        .filter_map(|r| {
            let kind = RegimeKind::from_str_opt(&r.regime)?;
            let ts = qtss_domain::v2::regime::TrendStrength::from_str_opt(
                r.trend_strength.as_deref().unwrap_or("none"),
            ).unwrap_or(qtss_domain::v2::regime::TrendStrength::None);
            Some((r.interval.clone(), qtss_domain::v2::regime::RegimeSnapshot {
                at: r.computed_at,
                kind,
                trend_strength: ts,
                adx: rust_decimal::Decimal::from_f64_retain(r.adx.unwrap_or(0.0)).unwrap_or_default(),
                bb_width: rust_decimal::Decimal::from_f64_retain(r.bb_width.unwrap_or(0.0)).unwrap_or_default(),
                atr_pct: rust_decimal::Decimal::from_f64_retain(r.atr_pct.unwrap_or(0.0)).unwrap_or_default(),
                choppiness: rust_decimal::Decimal::from_f64_retain(r.choppiness.unwrap_or(0.0)).unwrap_or_default(),
                confidence: r.confidence as f32,
            }))
        })
        .collect();

    let mtf = qtss_regime::multi_tf::compute_confluence(symbol, &snap_pairs, &tf_weights)?;
    Some(MultiTfRegimeContext {
        dominant_regime: mtf.dominant_regime,
        confluence_score: mtf.confluence_score,
        is_transitioning: mtf.is_transitioning,
    })
}

