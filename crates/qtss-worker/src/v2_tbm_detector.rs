//! v2 TBM (Top/Bottom Mining) reversal detector — Faz 7.5 Adım 3.
//!
//! TBM is a meta-detector: it doesn't read pivots like the other v2
//! detectors, it consumes a wide indicator bundle (stoch, MACD, EMA,
//! BB, MFI, OBV/CVD, ATR, fib proximity) and produces a *reversal
//! score* for both bottom and top hypotheses. When the score crosses
//! a configurable threshold the loop emits a row into
//! `qtss_v2_detections` with `family="tbm"` and `subkind` of
//! `bottom_setup` / `top_setup`, so the chart overlay and validator
//! both see it through the same pipeline as the structural detectors.
//!
//! ## Why a separate loop (not folded into v2_detection_orchestrator)
//!
//! The structural detectors all share a `(PivotTree, Instrument, TF,
//! Regime) → Option<Detection>` shape. TBM needs a *different* input
//! contract entirely (closes/highs/lows/volumes vectors plus indicator
//! bundle), so wedging it into the orchestrator's `DetectorRunner`
//! trait would either pollute that trait or force every other detector
//! to compute indicators it doesn't use. CLAUDE.md #3: keep layers
//! cleanly separated. CLAUDE.md #1: no scattered conditionals.
//!
//! ## Onchain pillar
//!
//! Disabled until Faz 7.7 (`tbm.onchain_enabled` defaults to false).
//! When the rewrite lands the loop will pull `OnchainMetrics` from the
//! new pipeline and feed `score_onchain` into the bundle.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use qtss_indicators::{
    bollinger::bollinger,
    cvd::cvd,
    ema::ema,
    macd::macd,
    mfi::mfi,
    obv::obv,
    stochastic::stochastic,
    volatility::atr,
};
use qtss_storage::{
    list_enabled_engine_symbols, list_recent_bars, resolve_system_f64, resolve_system_string,
    resolve_system_u64, resolve_worker_enabled_flag, EngineSymbolRow, NewDetection,
    V2DetectionRepository,
};
use qtss_tbm::{
    config::{TbmAnchorTuning, TbmConfig, TbmMtfTuning, TbmPillarWeights, TbmSetupTuning},
    momentum::score_momentum,
    onchain::{score_onchain, OnchainMetrics, OnchainMetricsProvider},
    pillar::{PillarKind, PillarScore},
    scorer::score_tbm,
    setup::{detect_setups, SetupDirection, SetupThresholds, TbmSetup},
    structure::score_structure,
    volume::score_volume,
};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use tracing::{debug, info, warn};
use uuid::Uuid;

const MIN_BARS: usize = 60;

pub async fn v2_tbm_detector_loop(
    pool: PgPool,
    onchain_provider: Arc<dyn OnchainMetricsProvider>,
) {
    info!("v2 tbm detector loop spawned (gated on tbm.enabled)");
    let repo = Arc::new(V2DetectionRepository::new(pool.clone()));

    loop {
        let cfg = load_config(&pool).await;

        if !cfg.enabled {
            tokio::time::sleep(Duration::from_secs(cfg.tick_interval_s)).await;
            continue;
        }

        let mode = resolve_runtime_mode(&pool).await;

        match run_pass(&pool, repo.clone(), &cfg, &mode, onchain_provider.clone()).await {
            Ok(stats) => {
                if stats.inserted > 0 || stats.processed > 0 {
                    info!(
                        processed = stats.processed,
                        emitted = stats.emitted,
                        inserted = stats.inserted,
                        skipped = stats.skipped,
                        "v2 tbm detector pass complete"
                    );
                } else {
                    debug!("v2 tbm detector pass: no enabled symbols");
                }
            }
            Err(e) => warn!(%e, "v2 tbm detector pass failed"),
        }

        tokio::time::sleep(Duration::from_secs(cfg.tick_interval_s)).await;
    }
}

// ---------------------------------------------------------------------
// Config hydration
// ---------------------------------------------------------------------

async fn load_config(pool: &PgPool) -> TbmConfig {
    let enabled = resolve_worker_enabled_flag(
        pool,
        "tbm",
        "enabled",
        "QTSS_TBM_ENABLED",
        false,
    )
    .await;
    let tick_interval_s = resolve_system_u64(
        pool,
        "tbm",
        "tick_interval_s",
        "QTSS_TBM_TICK_INTERVAL_S",
        60,
        5,
        3600,
    )
    .await;
    let lookback_bars = resolve_system_u64(
        pool,
        "tbm",
        "lookback_bars",
        "QTSS_TBM_LOOKBACK_BARS",
        300,
        60,
        5000,
    )
    .await as usize;
    let onchain_enabled = resolve_worker_enabled_flag(
        pool,
        "tbm",
        "onchain_enabled",
        "QTSS_TBM_ONCHAIN_ENABLED",
        false,
    )
    .await;

    let weights = TbmPillarWeights {
        momentum: resolve_system_f64(pool, "tbm", "pillar.momentum.weight", "QTSS_TBM_W_MOMENTUM", 0.30).await,
        volume: resolve_system_f64(pool, "tbm", "pillar.volume.weight", "QTSS_TBM_W_VOLUME", 0.25).await,
        structure: resolve_system_f64(pool, "tbm", "pillar.structure.weight", "QTSS_TBM_W_STRUCTURE", 0.30).await,
        onchain: resolve_system_f64(pool, "tbm", "pillar.onchain.weight", "QTSS_TBM_W_ONCHAIN", 0.15).await,
    };
    let setup = TbmSetupTuning {
        min_score: resolve_system_f64(pool, "tbm", "setup.min_score", "QTSS_TBM_MIN_SCORE", 50.0).await,
        min_active_pillars: resolve_system_u64(
            pool,
            "tbm",
            "setup.min_active_pillars",
            "QTSS_TBM_MIN_ACTIVE_PILLARS",
            2,
            0,
            10,
        )
        .await as usize,
        pillar_active_threshold: resolve_system_f64(
            pool,
            "tbm",
            "setup.pillar_active_threshold",
            "QTSS_TBM_PILLAR_ACTIVE_THRESHOLD",
            20.0,
        )
        .await,
        max_anchor_age_bars: resolve_system_u64(
            pool,
            "tbm",
            "setup.max_anchor_age_bars",
            "QTSS_TBM_MAX_ANCHOR_AGE_BARS",
            12,
            1,
            500,
        )
        .await as usize,
    };
    let mtf = TbmMtfTuning {
        required_confirms: resolve_system_u64(
            pool,
            "tbm",
            "mtf.required_confirms",
            "QTSS_TBM_MTF_REQUIRED",
            2,
            0,
            10,
        )
        .await as usize,
        min_alignment: resolve_system_f64(
            pool,
            "tbm",
            "mtf.min_alignment",
            "QTSS_TBM_MTF_MIN_ALIGN",
            0.5,
        )
        .await,
    };

    let anchor = TbmAnchorTuning {
        pivot_radius: resolve_system_u64(
            pool, "tbm", "anchor.pivot_radius", "QTSS_TBM_ANCHOR_PIV_RADIUS", 3, 1, 20,
        ).await as usize,
        min_right_bars: resolve_system_u64(
            pool, "tbm", "anchor.min_right_bars", "QTSS_TBM_ANCHOR_MIN_RIGHT", 3, 0, 50,
        ).await as usize,
        wick_min_ratio: resolve_system_f64(
            pool, "tbm", "anchor.wick_min_ratio", "QTSS_TBM_ANCHOR_WICK_MIN", 0.25,
        ).await,
        vol_min_ratio: resolve_system_f64(
            pool, "tbm", "anchor.vol_min_ratio", "QTSS_TBM_ANCHOR_VOL_MIN", 1.0,
        ).await,
    };

    TbmConfig {
        enabled,
        tick_interval_s,
        lookback_bars,
        weights,
        setup,
        mtf,
        anchor,
        onchain_enabled,
    }
}

async fn resolve_runtime_mode(pool: &PgPool) -> String {
    let mode = resolve_system_string(pool, "worker", "runtime_mode", "QTSS_RUNTIME_MODE", "live").await;
    match mode.as_str() {
        "live" | "dry" | "backtest" => mode,
        _ => "live".to_string(),
    }
}

// ---------------------------------------------------------------------
// Pass + per-symbol processing
// ---------------------------------------------------------------------

#[derive(Default)]
struct PassStats {
    processed: usize,
    emitted: usize,
    inserted: usize,
    skipped: usize,
}

async fn run_pass(
    pool: &PgPool,
    repo: Arc<V2DetectionRepository>,
    cfg: &TbmConfig,
    mode: &str,
    onchain_provider: Arc<dyn OnchainMetricsProvider>,
) -> anyhow::Result<PassStats> {
    let mut stats = PassStats::default();
    let symbols = list_enabled_engine_symbols(pool).await?;

    for sym in symbols {
        if !qtss_storage::is_backfill_ready(pool, sym.id).await {
            continue;
        }
        match process_symbol(pool, repo.clone(), &sym, cfg, mode, onchain_provider.clone()).await {
            Ok(s) => {
                stats.processed += 1;
                stats.emitted += s.emitted;
                stats.inserted += s.inserted;
                stats.skipped += s.skipped;
            }
            Err(e) => warn!(symbol = %sym.symbol, interval = %sym.interval, %e, "tbm process_symbol failed"),
        }
    }

    Ok(stats)
}

#[derive(Default)]
struct SymbolStats {
    emitted: usize,
    inserted: usize,
    skipped: usize,
}

async fn process_symbol(
    pool: &PgPool,
    repo: Arc<V2DetectionRepository>,
    sym: &EngineSymbolRow,
    cfg: &TbmConfig,
    mode: &str,
    onchain_provider: Arc<dyn OnchainMetricsProvider>,
) -> anyhow::Result<SymbolStats> {
    // Faz 7.7: pull a fresh OnchainMetrics row up front so we can pass it
    // to both the bottom and top scoring branches without re-fetching.
    let onchain_metrics = if cfg.onchain_enabled {
        onchain_provider.fetch(&sym.symbol).await
    } else {
        None
    };
    let mut stats = SymbolStats::default();

    let raw_bars = list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        cfg.lookback_bars as i64,
    )
    .await?;
    if raw_bars.len() < MIN_BARS {
        stats.skipped += 1;
        return Ok(stats);
    }

    // Chronological order — indicators expect oldest→newest.
    let mut chronological = raw_bars;
    chronological.reverse();

    let highs: Vec<f64> = chronological.iter().map(|r| r.high.to_string().parse().unwrap_or(0.0)).collect();
    let lows: Vec<f64> = chronological.iter().map(|r| r.low.to_string().parse().unwrap_or(0.0)).collect();
    let closes: Vec<f64> = chronological.iter().map(|r| r.close.to_string().parse().unwrap_or(0.0)).collect();
    let opens: Vec<f64> = chronological.iter().map(|r| r.open.to_string().parse().unwrap_or(0.0)).collect();
    let vols: Vec<f64> = chronological.iter().map(|r| r.volume.to_string().parse().unwrap_or(0.0)).collect();

    let n = closes.len();
    let last = n - 1;

    // P22 — invalidate stale/broken forming TBM detections BEFORE
    // emitting new ones. Without this pass, a bottom_setup from weeks
    // ago stays `forming` forever and the chart keeps rendering a label
    // at a price zone the market has long since left. Two triggers:
    //   1. Anchor bar is older than max_anchor_age_bars on this TF.
    //   2. Invalidation price is breached (long: low <= invalidation;
    //      short: high >= invalidation) — setup geometry is dead.
    invalidate_stale_forming(
        &repo,
        sym,
        &chronological,
        cfg.setup.max_anchor_age_bars,
    )
    .await?;

    // ----- Indicator series (whole window, evaluate per-direction) ---
    let stoch = stochastic(&highs, &lows, &closes, 14, 3);
    let macd_r = macd(&closes, 12, 26, 9);
    let bb = bollinger(&closes, 20, 2.0);
    let mfi_v = mfi(&highs, &lows, &closes, &vols, 14);
    let obv_v = obv(&closes, &vols);
    let cvd_v = cvd(&highs, &lows, &closes, &vols);
    let _atr_v = atr(&highs, &lows, &closes, 14);
    let ema_fast = ema(&closes, 9);
    let ema_slow = ema(&closes, 21);

    // P22c — scoring is now anchored to the structural extremum bar
    // for each direction, not the latest bar. Previously stoch/MACD/
    // MFI were all read at `last`, so a bottom_setup anchored to a dip
    // 30 bars ago still carried the LATEST (post-rally) indicator
    // state. Result: oversold dip showed stoch=65 Weak instead of
    // stoch=12 Strong. Now each direction evaluates indicators at its
    // own extremum bar (argmin(lows) for Bottom, argmax(highs) for Top)
    // within the same 50-bar window that feeds swing_high/low.
    let win_start = n.saturating_sub(50);
    let swing_high = highs[win_start..n].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let swing_low = lows[win_start..n].iter().cloned().fold(f64::INFINITY, f64::min);
    let last_close = closes[last];

    // P22f — structural anchor selection. argmin/argmax alone picked
    // mid-trend forming bars with no wick, no volume climax, no right-
    // bar confirmation. We now score pivot-low/high candidates on
    // (depth + rejection wick + volume climax + liquidity sweep) and
    // require `min_right_bars` of completed bars after the candidate.
    // Mirrors the Sweep→Rejection legs of the textbook reversal
    // framework; BoS/Retest is the confirmation state machine (P23).
    let bottom_anchor = pick_anchor(
        &opens, &highs, &lows, &closes, &vols,
        win_start, n, true, &cfg.anchor,
    );
    let top_anchor = pick_anchor(
        &opens, &highs, &lows, &closes, &vols,
        win_start, n, false, &cfg.anchor,
    );

    // P22e — supersede open forming detections whose stored anchor is
    // worse than the current argmin/argmax. Without this, a bottom_setup
    // from 3 hours ago at $60k sticks around even after price makes a
    // fresh $58k LL; the chart keeps rendering the stale anchor and
    // dedup_open blocks a new, correct emission. We invalidate the old
    // row (reason=superseded) so a freshly-anchored one can be inserted
    // this tick.
    supersede_outdated_forming(
        &repo, sym, &chronological, &lows, &highs, bottom_anchor, top_anchor,
    )
    .await?;

    let empty: Vec<(usize, f64)> = Vec::new();

    // Helper: take an indicator snapshot at `idx` (not `last`).
    let snapshot = |idx: usize| {
        let stoch_k = finite_or(stoch.k.get(idx).copied(), 50.0);
        let stoch_d = finite_or(stoch.d.get(idx).copied(), 50.0);
        let macd_hist = finite_or(macd_r.histogram.get(idx).copied(), 0.0);
        let macd_hist_prev = finite_or(
            macd_r.histogram.get(idx.saturating_sub(1)).copied(),
            0.0,
        );
        let ema_fast_v = finite_or(ema_fast.get(idx).copied(), closes[idx]);
        let ema_slow_v = finite_or(ema_slow.get(idx).copied(), closes[idx]);
        let bb_pct_b = finite_or(bb.percent_b.get(idx).copied(), 0.5);
        let bb_squeeze = bb
            .bandwidth
            .get(idx)
            .copied()
            .map(|w| w.is_finite() && w < 0.05)
            .unwrap_or(false);
        let mfi_val = finite_or(mfi_v.get(idx).copied(), 50.0);
        // P22d-div — pass the actual OBV/CVD/price window ending at idx
        // so score_volume can do a real half-window swing comparison
        // (divergence) instead of just reading the sign of a slope.
        let win_lo = idx.saturating_sub(19);
        let price_window: Vec<f64> = closes[win_lo..=idx].to_vec();
        let obv_window: Vec<f64> = obv_v[win_lo..=idx].to_vec();
        let cvd_window: Vec<f64> = cvd_v[win_lo..=idx].to_vec();
        let vol_at = vols[idx];
        let vol_start = idx.saturating_sub(20);
        let vol_window: Vec<f64> = vols[vol_start..=idx].to_vec();
        let vol_avg_v = window_mean(&vol_window, vol_window.len());
        let close_at = closes[idx];
        let (fib_prox, fib_name) = nearest_fib(swing_high, swing_low, close_at);
        (
            stoch_k, stoch_d, macd_hist, macd_hist_prev,
            ema_fast_v, ema_slow_v, bb_pct_b, bb_squeeze,
            mfi_val, price_window, obv_window, cvd_window,
            vol_at, vol_avg_v, fib_prox, fib_name,
        )
    };

    let (
        b_stoch_k, b_stoch_d, b_macd_h, b_macd_hp,
        b_ema_f, b_ema_s, b_bb_pb, b_bb_sq,
        b_mfi, b_price_w, b_obv_w, b_cvd_w,
        b_vol, b_vol_avg, b_fib_p, b_fib_n,
    ) = snapshot(bottom_anchor);
    let (
        t_stoch_k, t_stoch_d, t_macd_h, t_macd_hp,
        t_ema_f, t_ema_s, t_bb_pb, t_bb_sq,
        t_mfi, t_price_w, t_obv_w, t_cvd_w,
        t_vol, t_vol_avg, t_fib_p, t_fib_n,
    ) = snapshot(top_anchor);

    // ----- Pillar evaluation (bottom + top, each anchored) -----------
    let bottom = build_score(
        cfg,
        true,
        b_stoch_k, b_stoch_d, b_macd_h, b_macd_hp,
        b_ema_f, b_ema_s,
        &empty, &empty, &empty, &empty,
        b_mfi, &b_price_w, &b_obv_w, &b_cvd_w, b_vol, b_vol_avg,
        b_fib_p, b_fib_n, b_bb_pb, b_bb_sq,
        onchain_metrics.as_ref(),
    );
    let top = build_score(
        cfg,
        false,
        t_stoch_k, t_stoch_d, t_macd_h, t_macd_hp,
        t_ema_f, t_ema_s,
        &empty, &empty, &empty, &empty,
        t_mfi, &t_price_w, &t_obv_w, &t_cvd_w, t_vol, t_vol_avg,
        t_fib_p, t_fib_n, t_bb_pb, t_bb_sq,
        onchain_metrics.as_ref(),
    );

    let thresholds = SetupThresholds {
        min_score: cfg.setup.min_score,
        min_active_pillars: cfg.setup.min_active_pillars,
    };
    let setups = detect_setups(&bottom, &top, &thresholds);
    if setups.is_empty() {
        return Ok(stats);
    }

    // ----- Persist to qtss_v2_detections -----------------------------
    let invalidation = invalidation_for(swing_high, swing_low, last_close);

    for setup in &setups {
        stats.emitted += 1;

        // P21/P22c — reuse the anchor indices already computed above
        // (same argmin/argmax that fed the indicator snapshot).
        let (anchor_idx, anchor_px_f64) = match setup.direction {
            SetupDirection::Bottom => (bottom_anchor, lows[bottom_anchor]),
            SetupDirection::Top => (top_anchor, highs[top_anchor]),
        };
        let anchor_bar_time = chronological[anchor_idx].open_time;

        // P22e — dedup now keys on (subkind, anchor_bar_time). A new
        // anchor bar means a new structural extremum → new row.
        if dedup_open(&repo, sym, setup, anchor_bar_time).await? {
            continue;
        }

        let subkind = subkind_for(setup.direction);
        let anchor_price = Decimal::from_f64(anchor_px_f64).unwrap_or(Decimal::ZERO);
        let invalidation_price = Decimal::from_f64(invalidation).unwrap_or(Decimal::ZERO);

        let anchors = json!([
            {
                "bar_index": anchor_idx,
                "time": anchor_bar_time.to_rfc3339(),
                "price": anchor_price.to_string(),
                "level": "Setup",
                "label": format!("{:?}", setup.signal),
            }
        ]);
        let regime = json!({});
        let raw_meta = json!({
            "tbm_score": setup.score,
            "signal": format!("{:?}", setup.signal),
            "pillars": pillar_meta(&bottom, &top, setup.direction),
            "details": setup.pillar_details,
        });

        let row = NewDetection {
            id: Uuid::new_v4(),
            detected_at: Utc::now(),
            exchange: &sym.exchange,
            symbol: &sym.symbol,
            timeframe: &sym.interval,
            family: "tbm",
            subkind,
            state: "forming",
            structural_score: (setup.score / 100.0) as f32,
            invalidation_price,
            anchors,
            regime,
            raw_meta,
            mode,
        };
        repo.insert(row).await?;
        stats.inserted += 1;
    }

    Ok(stats)
}

// ---------------------------------------------------------------------
// Scoring helpers
// ---------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn build_score(
    cfg: &TbmConfig,
    is_bottom: bool,
    stoch_k: f64,
    stoch_d: f64,
    macd_hist: f64,
    macd_hist_prev: f64,
    ema_fast_last: f64,
    ema_slow_last: f64,
    price_high_pivots: &[(usize, f64)],
    price_low_pivots: &[(usize, f64)],
    indicator_high_pivots: &[(usize, f64)],
    indicator_low_pivots: &[(usize, f64)],
    mfi_last: f64,
    price_window: &[f64],
    obv_window: &[f64],
    cvd_window: &[f64],
    vol_last: f64,
    vol_avg: f64,
    fib_proximity: f64,
    fib_level_name: &str,
    bb_pct_b: f64,
    bb_squeeze: bool,
    onchain_metrics: Option<&OnchainMetrics>,
) -> qtss_tbm::TbmScore {
    let mut momentum = score_momentum(
        stoch_k,
        stoch_d,
        macd_hist,
        macd_hist_prev,
        ema_fast_last,
        ema_slow_last,
        price_high_pivots,
        price_low_pivots,
        indicator_high_pivots,
        indicator_low_pivots,
        is_bottom,
    );
    momentum.weight = cfg.weights.momentum;

    let mut volume = score_volume(
        mfi_last,
        price_window,
        obv_window,
        cvd_window,
        vol_last,
        vol_avg,
        is_bottom,
    );
    volume.weight = cfg.weights.volume;

    let mut structure = score_structure(
        fib_proximity,
        fib_level_name,
        bb_pct_b,
        bb_squeeze,
        false,
        0.0,
        "",
        is_bottom,
    );
    structure.weight = cfg.weights.structure;

    // Faz 7.7: pull score from the v2 onchain bridge when both the
    // master flag is on and the bridge returned a fresh row. Otherwise
    // emit a zero-weight placeholder so the pillar collapses out of the
    // weighted denominator instead of dragging the total down.
    let mut onchain = match (cfg.onchain_enabled, onchain_metrics) {
        (true, Some(m)) => score_onchain(m, is_bottom),
        _ => PillarScore {
            kind: PillarKind::Onchain,
            score: 0.0,
            weight: 0.0,
            details: vec!["onchain disabled or no fresh data".into()],
        },
    };
    if onchain.weight > 0.0 {
        onchain.weight = cfg.weights.onchain;
    }

    score_tbm(vec![momentum, volume, structure, onchain])
}

fn pillar_meta(
    bottom: &qtss_tbm::TbmScore,
    top: &qtss_tbm::TbmScore,
    dir: SetupDirection,
) -> serde_json::Value {
    let src = match dir {
        SetupDirection::Bottom => bottom,
        SetupDirection::Top => top,
    };
    let arr: Vec<_> = src
        .pillars
        .iter()
        .map(|p| {
            json!({
                "kind": format!("{:?}", p.kind),
                "score": p.score,
                "weight": p.weight,
            })
        })
        .collect();
    json!({ "total": src.total, "pillars": arr })
}

fn subkind_for(dir: SetupDirection) -> &'static str {
    match dir {
        SetupDirection::Bottom => "bottom_setup",
        SetupDirection::Top => "top_setup",
    }
}

fn invalidation_for(swing_high: f64, swing_low: f64, last_close: f64) -> f64 {
    // Bottom setup invalidates below swing low; top above swing high.
    // Without the direction in scope here we pick whichever extreme is
    // *opposite* the close and let the validator interpret it per-row.
    if (last_close - swing_low).abs() < (swing_high - last_close).abs() {
        swing_low * 0.99
    } else {
        swing_high * 1.01
    }
}

/// P22 — sweep forming TBM detections for this symbol/TF and
/// invalidate any whose anchor bar is older than `max_age_bars`
/// measured against the latest bar, or whose invalidation_price has
/// been breached by price action since the anchor.
async fn invalidate_stale_forming(
    repo: &V2DetectionRepository,
    sym: &EngineSymbolRow,
    chronological: &[qtss_storage::MarketBarRow],
    max_age_bars: usize,
) -> anyhow::Result<()> {
    use qtss_storage::DetectionFilter;
    let rows = repo
        .list_filtered(DetectionFilter {
            exchange: Some(&sym.exchange),
            symbol: Some(&sym.symbol),
            timeframe: Some(&sym.interval),
            family: Some("tbm"),
            state: Some("forming"),
            mode: None,
            limit: 50,
        })
        .await?;
    if rows.is_empty() { return Ok(()); }

    let latest_time = chronological
        .last()
        .map(|b| b.open_time)
        .unwrap_or_else(Utc::now);
    // Precompute lookup: open_time → index
    for row in rows {
        // Extract anchor bar_time from first anchor entry.
        let anchor_time: Option<chrono::DateTime<Utc>> = row
            .anchors
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.get("time"))
            .and_then(|t| t.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let anchor_idx = anchor_time.and_then(|t| {
            chronological.iter().position(|b| b.open_time >= t)
        });
        let latest_idx = chronological.len().saturating_sub(1);
        let age_bars = anchor_idx
            .map(|i| latest_idx.saturating_sub(i))
            .unwrap_or(usize::MAX);

        // Age gate
        let aged_out = age_bars > max_age_bars;

        // Invalidation-price gate: scan bars since anchor for breach.
        let inv_px = row.invalidation_price.to_string().parse::<f64>().ok();
        let breached = match (anchor_idx, inv_px, row.subkind.as_str()) {
            (Some(start), Some(inv), "bottom_setup") => {
                chronological[start..].iter().any(|b| {
                    b.low.to_string().parse::<f64>().unwrap_or(f64::INFINITY) <= inv
                })
            }
            (Some(start), Some(inv), "top_setup") => {
                chronological[start..].iter().any(|b| {
                    b.high.to_string().parse::<f64>().unwrap_or(f64::NEG_INFINITY) >= inv
                })
            }
            _ => false,
        };

        if aged_out || breached {
            let _ = latest_time; // kept for potential future logging
            repo.update_state(row.id, "invalidated").await?;
            debug!(
                id = %row.id,
                subkind = %row.subkind,
                age_bars,
                breached,
                "P22: tbm forming detection invalidated"
            );
        }
    }
    Ok(())
}

/// P22e — dedup key is now (subkind, anchor_bar_time). Previously we
/// blocked any new same-subkind emission that arrived after the last
/// bar's open_time, which meant a re-anchored setup at a *new* extremum
/// got silently dropped because an older forming row existed. The
/// supersede pass invalidates the older row; this function only
/// short-circuits when the same anchor bar already has an open row —
/// i.e. re-emissions within the same bar.
async fn dedup_open(
    repo: &V2DetectionRepository,
    sym: &EngineSymbolRow,
    setup: &TbmSetup,
    anchor_bar_time: chrono::DateTime<Utc>,
) -> anyhow::Result<bool> {
    use qtss_storage::DetectionFilter;
    let rows = repo
        .list_filtered(DetectionFilter {
            exchange: Some(&sym.exchange),
            symbol: Some(&sym.symbol),
            timeframe: Some(&sym.interval),
            family: Some("tbm"),
            state: Some("forming"),
            mode: None,
            limit: 20,
        })
        .await?;
    let target_subkind = subkind_for(setup.direction);
    for row in rows {
        if row.subkind != target_subkind { continue; }
        let stored_anchor = row
            .anchors
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.get("time"))
            .and_then(|t| t.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        if stored_anchor == Some(anchor_bar_time) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// P22e — invalidate any open forming TBM row whose stored anchor is
/// *worse* than the current tick's argmin/argmax. "Worse" means: for a
/// bottom_setup the current argmin(lows) is lower than the stored
/// anchor price (market made a fresh LL), and mirror for tops. Uses a
/// 0.1% tolerance so tiny floating-point / rounding noise doesn't flip
/// rows needlessly. The row that owns the current anchor bar is left
/// alone (stored_time == current_time short-circuit).
async fn supersede_outdated_forming(
    repo: &V2DetectionRepository,
    sym: &EngineSymbolRow,
    chronological: &[qtss_storage::MarketBarRow],
    lows: &[f64],
    highs: &[f64],
    bottom_anchor: usize,
    top_anchor: usize,
) -> anyhow::Result<()> {
    use qtss_storage::DetectionFilter;
    let rows = repo
        .list_filtered(DetectionFilter {
            exchange: Some(&sym.exchange),
            symbol: Some(&sym.symbol),
            timeframe: Some(&sym.interval),
            family: Some("tbm"),
            state: Some("forming"),
            mode: None,
            limit: 50,
        })
        .await?;
    if rows.is_empty() { return Ok(()); }

    let bottom_px = lows[bottom_anchor];
    let bottom_time = chronological[bottom_anchor].open_time;
    let top_px = highs[top_anchor];
    let top_time = chronological[top_anchor].open_time;

    for row in rows {
        let stored_px: Option<f64> = row
            .anchors
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.get("price"))
            .and_then(|p| p.as_str())
            .and_then(|s| s.parse::<f64>().ok());
        let stored_time: Option<chrono::DateTime<Utc>> = row
            .anchors
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.get("time"))
            .and_then(|t| t.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let (better, curr_time) = match row.subkind.as_str() {
            "bottom_setup" => {
                let b = stored_px.map(|sp| bottom_px < sp * 0.999).unwrap_or(false);
                (b, bottom_time)
            }
            "top_setup" => {
                let b = stored_px.map(|sp| top_px > sp * 1.001).unwrap_or(false);
                (b, top_time)
            }
            _ => (false, Utc::now()),
        };

        if stored_time == Some(curr_time) { continue; }
        if !better { continue; }

        repo.update_state(row.id, "invalidated").await?;
        debug!(
            id = %row.id,
            subkind = %row.subkind,
            "P22e: tbm forming superseded by better extremum"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Math helpers
// ---------------------------------------------------------------------

fn finite_or(v: Option<f64>, fallback: f64) -> f64 {
    match v {
        Some(x) if x.is_finite() => x,
        _ => fallback,
    }
}

#[allow(dead_code)]
fn slope_last_n(series: &[f64], n: usize) -> f64 {
    if series.len() < n + 1 {
        return 0.0;
    }
    let last = series[series.len() - 1];
    let prev = series[series.len() - 1 - n];
    if !last.is_finite() || !prev.is_finite() {
        return 0.0;
    }
    last - prev
}

fn window_mean(series: &[f64], n: usize) -> f64 {
    if series.is_empty() {
        return 0.0;
    }
    let start = series.len().saturating_sub(n);
    let win = &series[start..];
    let count = win.iter().filter(|v| v.is_finite()).count();
    if count == 0 {
        return 0.0;
    }
    let sum: f64 = win.iter().filter(|v| v.is_finite()).sum();
    sum / count as f64
}

/// Distance from `price` to the nearest standard fib retracement level
/// of the (swing_high, swing_low) range. Returns proximity in 0..1
/// (1 = price sits exactly on a level) plus the level label.
fn nearest_fib(swing_high: f64, swing_low: f64, price: f64) -> (f64, &'static str) {
    if !(swing_high.is_finite() && swing_low.is_finite()) || swing_high <= swing_low {
        return (0.0, "");
    }
    let range = swing_high - swing_low;
    let levels: &[(f64, &'static str)] = &[
        (0.236, "23.6%"),
        (0.382, "38.2%"),
        (0.5, "50%"),
        (0.618, "61.8%"),
        (0.786, "78.6%"),
    ];
    let mut best = (0.0_f64, "");
    for (ratio, label) in levels {
        let level_price = swing_high - ratio * range;
        let dist = (price - level_price).abs() / range;
        let proximity = (1.0 - dist * 5.0).clamp(0.0, 1.0);
        if proximity > best.0 {
            best = (proximity, *label);
        }
    }
    best
}

// ---------------------------------------------------------------------
// P22f — structural anchor picker (Sweep + Rejection + Confirmation)
// ---------------------------------------------------------------------
//
// Ranks pivot-low (or pivot-high) candidates in the window by a
// composite score and returns the best index. Composite terms:
//
//   depth       — how close the candidate is to the window extreme.
//                 Full weight when it IS the extreme, decays linearly
//                 to 0 at the opposite side. Always required; a
//                 mid-range pivot is not a reversal anchor.
//   wick        — rejection wick ratio (lower wick / total range for
//                 bottom, upper wick for top). Strong rejection = 1.
//                 Gated by `wick_min_ratio`.
//   volume      — volume / 20-bar avg, clamped [0, 1] above threshold.
//                 Climactic volume at a pivot = reversal tell.
//   sweep       — bonus when the candidate prints a fresh LL/HH that
//                 takes out a prior window low/high by at least 0.05%,
//                 i.e. a textbook liquidity sweep.
//
// Gates:
//   * Must be a pivot extreme over ±pivot_radius bars.
//   * Must have at least `min_right_bars` completed bars AFTER it
//     (keeps the picker off the currently forming bar).
//
// Fallback: plain argmin/argmax if no candidate clears the gates —
// early bars, very short history.
fn pick_anchor(
    opens: &[f64],
    highs: &[f64],
    lows: &[f64],
    _closes: &[f64],
    vols: &[f64],
    win_start: usize,
    n: usize,
    is_bottom: bool,
    cfg: &TbmAnchorTuning,
) -> usize {
    let last = n.saturating_sub(1);
    let fallback = if is_bottom {
        lows[win_start..n]
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| win_start + i)
            .unwrap_or(last)
    } else {
        highs[win_start..n]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| win_start + i)
            .unwrap_or(last)
    };

    let r = cfg.pivot_radius.max(1);
    let right = cfg.min_right_bars;
    let lo_start = win_start + r;
    let hi_end = last.saturating_sub(right);
    if lo_start > hi_end {
        return fallback;
    }

    let win_hi = highs[win_start..n].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let win_lo = lows[win_start..n].iter().cloned().fold(f64::INFINITY, f64::min);
    let win_range = (win_hi - win_lo).max(1e-10);

    let mut best_idx: Option<usize> = None;
    let mut best_score = f64::NEG_INFINITY;

    for i in lo_start..=hi_end {
        let l = i.saturating_sub(r);
        let h = (i + r).min(last);
        let range_i = (highs[i] - lows[i]).max(1e-10);

        // Pivot gate
        let is_pivot = if is_bottom {
            let local_min = lows[l..=h].iter().cloned().fold(f64::INFINITY, f64::min);
            (lows[i] - local_min).abs() < 1e-9
        } else {
            let local_max = highs[l..=h].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            (highs[i] - local_max).abs() < 1e-9
        };
        if !is_pivot { continue; }

        // Wick gate + term
        let body_hi = opens[i].max(_closes[i]);
        let body_lo = opens[i].min(_closes[i]);
        let wick_ratio = if is_bottom {
            ((body_lo - lows[i]).max(0.0)) / range_i
        } else {
            ((highs[i] - body_hi).max(0.0)) / range_i
        };
        if wick_ratio < cfg.wick_min_ratio { continue; }

        // Depth term (linear to window extreme)
        let depth = if is_bottom {
            (win_hi - lows[i]) / win_range
        } else {
            (highs[i] - win_lo) / win_range
        };

        // Volume term
        let v_start = i.saturating_sub(20).max(win_start);
        let v_count = (i - v_start).max(1) as f64;
        let v_avg = vols[v_start..i].iter().sum::<f64>() / v_count;
        let v_ratio = if v_avg > 0.0 { vols[i] / v_avg } else { 1.0 };
        let vol_term = ((v_ratio - cfg.vol_min_ratio).max(0.0) / 2.0).min(1.0);

        // Sweep term — did this bar take out a prior window extreme?
        let sweep = if is_bottom {
            let prior_lo = lows[win_start..i].iter().cloned().fold(f64::INFINITY, f64::min);
            if prior_lo.is_finite() && lows[i] < prior_lo * 0.9995 { 1.0 } else { 0.0 }
        } else {
            let prior_hi = highs[win_start..i].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            if prior_hi.is_finite() && highs[i] > prior_hi * 1.0005 { 1.0 } else { 0.0 }
        };

        // Composite. Depth is the dominant term so we still prefer the
        // actual window extreme when multiple pivots qualify; wick and
        // volume break ties and demote low-quality pivots.
        let score = depth * 2.0 + wick_ratio * 1.0 + vol_term * 1.0 + sweep * 0.75;
        if score > best_score {
            best_score = score;
            best_idx = Some(i);
        }
    }

    best_idx.unwrap_or(fallback)
}
