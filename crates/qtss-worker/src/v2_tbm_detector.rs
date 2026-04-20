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
    config::{TbmAnchorTuning, TbmConfig, TbmConfirmTuning, TbmMtfTuning, TbmPillarWeights, TbmSetupTuning},
    momentum::score_momentum,
    onchain::{score_onchain, OnchainMetrics, OnchainMetricsProvider},
    pillar::{PillarKind, PillarScore},
    scorer::score_tbm,
    setup::{detect_setups, SetupDirection, SetupThresholds},
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
        // P26 — HTF parent ladder. Stored as CSV pairs
        // "ltf:htf,ltf:htf,..." in system_config so operators can retune
        // the MTF ladder without a deploy.
        htf_parents: parse_htf_parents(
            &resolve_system_string(
                pool, "tbm", "mtf.htf_parents", "QTSS_TBM_HTF_PARENTS",
                "1m:5m,5m:15m,15m:1h,1h:4h,4h:1d,1d:1w",
            ).await,
        ),
    };

    let confirm = TbmConfirmTuning {
        bos_required: resolve_system_u64(
            pool, "tbm", "confirm.bos_required", "QTSS_TBM_CONFIRM_BOS", 1, 0, 1,
        ).await != 0,
        window_bars: resolve_system_u64(
            pool, "tbm", "confirm.window_bars", "QTSS_TBM_CONFIRM_WINDOW", 8, 1, 100,
        ).await as usize,
        followthrough_atr_mult: resolve_system_f64(
            pool, "tbm", "confirm.followthrough_atr_mult", "QTSS_TBM_CONFIRM_FT_ATR", 1.0,
        ).await,
        followthrough_bars: resolve_system_u64(
            pool, "tbm", "confirm.followthrough_bars", "QTSS_TBM_CONFIRM_FT_BARS", 3, 1, 20,
        ).await as usize,
        retest_max_age_bars: resolve_system_u64(
            pool, "tbm", "confirm.retest_max_age_bars", "QTSS_TBM_RETEST_MAX_AGE", 12, 1, 100,
        ).await as usize,
        retest_proximity_atr: resolve_system_f64(
            pool, "tbm", "confirm.retest_proximity_atr", "QTSS_TBM_RETEST_PROX_ATR", 0.5,
        ).await,
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
        sweep_required: resolve_system_u64(
            pool, "tbm", "anchor.sweep_required", "QTSS_TBM_ANCHOR_SWEEP_REQ", 0, 0, 1,
        ).await != 0,
        equal_level_tol: resolve_system_f64(
            pool, "tbm", "anchor.equal_level_tol", "QTSS_TBM_ANCHOR_EQLVL_TOL", 0.002,
        ).await,
        equal_level_min_touches: resolve_system_u64(
            pool, "tbm", "anchor.equal_level_min_touches", "QTSS_TBM_ANCHOR_EQLVL_MIN", 1, 0, 10,
        ).await as usize,
        equal_level_required: resolve_system_u64(
            pool, "tbm", "anchor.equal_level_required", "QTSS_TBM_ANCHOR_EQLVL_REQ", 0, 0, 1,
        ).await != 0,
    };

    let effort_result = qtss_tbm::TbmEffortResultTuning {
        enabled: resolve_system_u64(
            pool, "tbm", "effort_result.enabled", "QTSS_TBM_EFFORT_ENABLED", 1, 0, 1,
        ).await != 0,
        scan_bars: resolve_system_u64(
            pool, "tbm", "effort_result.scan_bars", "QTSS_TBM_EFFORT_SCAN", 8, 2, 50,
        ).await as usize,
        range_small_ratio: resolve_system_f64(
            pool, "tbm", "effort_result.range_small_ratio", "QTSS_TBM_EFFORT_RNG", 0.7,
        ).await,
        vol_low_ratio: resolve_system_f64(
            pool, "tbm", "effort_result.vol_low_ratio", "QTSS_TBM_EFFORT_VOL_LO", 0.8,
        ).await,
        vol_high_ratio: resolve_system_f64(
            pool, "tbm", "effort_result.vol_high_ratio", "QTSS_TBM_EFFORT_VOL_HI", 1.5,
        ).await,
        no_supply_demand_pts: resolve_system_f64(
            pool, "tbm", "effort_result.no_supply_demand_pts", "QTSS_TBM_EFFORT_NSD_PTS", 10.0,
        ).await,
        absorption_pts: resolve_system_f64(
            pool, "tbm", "effort_result.absorption_pts", "QTSS_TBM_EFFORT_ABS_PTS", 15.0,
        ).await,
        max_bonus_pts: resolve_system_f64(
            pool, "tbm", "effort_result.max_bonus_pts", "QTSS_TBM_EFFORT_MAX", 25.0,
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
        confirm,
        effort_result,
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
    // P29c — route the detector's analysis TF into the bridge so a 15m
    // setup reads the ltf bucket (fast readings only) while a 4h+ setup
    // reads the htf bucket (full macro blend). Unknown intervals fall
    // back to tf_s=0 → htf via the bridge's legacy semantics.
    let tf_s = interval_to_seconds(&sym.interval).unwrap_or(0);
    let onchain_metrics = if cfg.onchain_enabled {
        onchain_provider.fetch_for_tf(&sym.symbol, tf_s).await
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

    // Faz 9 follow-up — no explicit supersede pass. Anchor refresh is
    // now handled in-place by the single-record upsert below: when the
    // current argmin/argmax differs from the stored anchor on a forming
    // row, update_anchor_projection rewrites the anchor block on the
    // same row instead of invalidating + re-inserting.

    // P23a+b — confirmation state machine. For every forming row:
    //   BoS?  yes → follow-through? yes → confirmed; no  → stay forming.
    //   age > window_bars AND no BoS → invalidated(timeout).
    // BoS = close breaks the structural level on the opposite side of
    // the anchor (for a bottom: pre-anchor swing high; for a top:
    // pre-anchor swing low). Follow-through = >= atr_mult * ATR(14)
    // close in the reversal direction within followthrough_bars of
    // the BoS bar. Keeps detections from sitting on "forming" forever
    // without ever proving the reversal.
    if cfg.confirm.bos_required {
        confirm_forming_or_timeout(
            &repo, sym, &chronological, &highs, &lows, &closes,
            &cfg.confirm,
        )
        .await?;
    }

    // P23c — retest detection. For every `confirmed` row that doesn't
    // yet carry a retest meta field, scan bars after the stored BoS
    // bar for a textbook pullback: for a bottom that's the first bar
    // whose low comes within `retest_proximity_atr × ATR` of the
    // broken pre-anchor swing high AND closes above it (HL); for a
    // top the mirror (LH). We stop looking after `retest_max_age_bars`.
    // The resulting bar is written into raw_meta so the chart can
    // render the entry zone — textbook "en güvenli giriş" point.
    detect_retest_for_confirmed(
        &repo, sym, &chronological, &highs, &lows, &closes, &cfg.confirm,
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

    // P27 — price & indicator pivots for momentum divergence. Previously
    // the detector passed empty pivot slices, killing a potential 50 pts
    // of regular+hidden divergence in the Momentum pillar. We now derive
    // pivots over the anchor window from `closes` (price) and the MACD
    // histogram (indicator; the classic divergence pair). Pivot radius
    // reuses cfg.anchor.pivot_radius so tuning stays in one place.
    let piv_r = cfg.anchor.pivot_radius.max(1);
    let (price_hi_pivots, price_lo_pivots) = compute_pivots(&closes, piv_r);
    let (ind_hi_pivots, ind_lo_pivots) = compute_pivots(&macd_r.histogram, piv_r);

    // ----- Pillar evaluation (bottom + top, each anchored) -----------
    let bottom = build_score(
        cfg,
        true,
        b_stoch_k, b_stoch_d, b_macd_h, b_macd_hp,
        b_ema_f, b_ema_s,
        &price_hi_pivots, &price_lo_pivots, &ind_hi_pivots, &ind_lo_pivots,
        b_mfi, &b_price_w, &b_obv_w, &b_cvd_w, b_vol, b_vol_avg,
        b_fib_p, b_fib_n, b_bb_pb, b_bb_sq,
        onchain_metrics.as_ref(),
    );
    let top = build_score(
        cfg,
        false,
        t_stoch_k, t_stoch_d, t_macd_h, t_macd_hp,
        t_ema_f, t_ema_s,
        &price_hi_pivots, &price_lo_pivots, &ind_hi_pivots, &ind_lo_pivots,
        t_mfi, &t_price_w, &t_obv_w, &t_cvd_w, t_vol, t_vol_avg,
        t_fib_p, t_fib_n, t_bb_pb, t_bb_sq,
        onchain_metrics.as_ref(),
    );
    // Keep _empty for the day one of the remaining call sites needs it.
    let _ = &empty;

    // P24 — Effort vs Result (Wyckoff volume law). Scan bars ending at
    // each anchor for no-supply / no-demand / absorption tells and add
    // a capped bonus to the volume pillar. Details are surfaced to
    // raw_meta via pillar_details.
    let mut bottom = bottom;
    let mut top = top;
    if cfg.effort_result.enabled {
        apply_effort_result(
            &mut bottom, bottom_anchor, &opens, &highs, &lows, &closes, &vols, true,
            &cfg.effort_result,
        );
        apply_effort_result(
            &mut top, top_anchor, &opens, &highs, &lows, &closes, &vols, false,
            &cfg.effort_result,
        );
    }

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
        // P26 — HTF confluence lookup. If the configured parent TF
        // carries a same-direction TBM row in forming/confirmed state,
        // annotate this (LTF) row with an htf_confluence block so the
        // GUI/risk-allocator can tier "sniper entries" (HTF level +
        // LTF sweep + BoS).
        let htf_confluence = if let Some(parent_tf) = cfg.mtf.htf_parents.get(&sym.interval) {
            fetch_htf_confluence(&repo, sym, parent_tf, subkind_for(setup.direction)).await
        } else {
            None
        };

        // P23d — Reversal Confidence Checklist (0–5). Seed sweep /
        // rejection / volume flags from the anchor bar now; bos and
        // retest flip true later in confirm_forming_or_timeout and
        // detect_retest_for_confirmed. See apply_reversal_checklist.
        let is_bottom_dir = matches!(setup.direction, SetupDirection::Bottom);
        let (anchor_sweep, anchor_rejection, anchor_volume) = anchor_flags(
            &opens, &highs, &lows, &closes, &vols,
            win_start, n, anchor_idx, is_bottom_dir, &cfg.anchor,
        );
        let mut raw_meta = json!({
            "tbm_score": setup.score,
            "signal": format!("{:?}", setup.signal),
            "pillars": pillar_meta(&bottom, &top, setup.direction),
            "details": setup.pillar_details,
            "reversal_checklist": {
                "flags": {
                    "sweep": anchor_sweep,
                    "rejection": anchor_rejection,
                    "volume": anchor_volume,
                    "bos": false,
                    "retest": false,
                },
            },
        });
        if let Some(htf) = htf_confluence {
            if let Some(obj) = raw_meta.as_object_mut() {
                obj.insert("htf_confluence".into(), htf);
            }
        }
        apply_reversal_checklist(&mut raw_meta);

        // Faz 9 follow-up — TBM single-record upsert.
        //
        // A TBM setup (bottom_setup / top_setup) is a *single logical
        // entity* per (exchange, symbol, timeframe, subkind). Anchors
        // get refined as the market prints a fresh extremum, scores
        // drift as pillars re-evaluate, and eventually the row either
        // confirms (BoS + follow-through) or gets invalidated. Inserting
        // a brand-new row on each anchor refresh left the Detections
        // panel with visual duplicates (e.g. 45% / 44% Weak sitting
        // side-by-side). Instead, we look up the *one* open row for
        // this key and:
        //
        //   - update anchors / score / raw_meta in place if it's still
        //     forming (anchor may have moved to a fresher extremum;
        //     that's expected evolution, not a new record),
        //   - only refresh score + raw_meta for confirmed / entry_ready
        //     rows (anchors are locked once BoS prints),
        //   - insert a new row only when no open row exists.
        //
        // If legacy duplicates from before this patch are present, we
        // keep the newest and flip the rest to `invalidated` so the
        // panel converges on a single live record.
        let open_rows = repo
            .list_open_by_key(
                &sym.exchange,
                &sym.symbol,
                &sym.interval,
                "tbm",
                subkind,
            )
            .await?;

        let structural_score = (setup.score / 100.0) as f32;

        if let Some((primary, duplicates)) = open_rows.split_first() {
            // Retire legacy duplicates (if any) first so the panel is
            // left with exactly one live row per key.
            for dup in duplicates {
                let _ = repo.update_state(dup.id, "invalidated").await;
                debug!(
                    id = %dup.id,
                    subkind = %dup.subkind,
                    "tbm single-record: legacy duplicate invalidated"
                );
            }

            // Preserve fields that the lifecycle has already enriched:
            // BoS / follow-through / retest / reversal checklist bits
            // all live in raw_meta, and anchors get locked on confirm.
            let locked = matches!(primary.state.as_str(), "confirmed" | "entry_ready");
            let merged_meta = merge_tbm_raw_meta(&primary.raw_meta, &raw_meta);
            let next_anchors = if locked { primary.anchors.clone() } else { anchors };
            let next_invalidation = if locked {
                primary.invalidation_price
            } else {
                invalidation_price
            };

            let _ = repo
                .update_anchor_projection(
                    primary.id,
                    structural_score,
                    next_invalidation,
                    next_anchors,
                    merged_meta,
                )
                .await;
            stats.inserted += 1; // counts as a successful emission cycle
        } else {
            let row = NewDetection {
                id: Uuid::new_v4(),
                detected_at: Utc::now(),
                exchange: &sym.exchange,
                symbol: &sym.symbol,
                timeframe: &sym.interval,
                family: "tbm",
                subkind,
                state: "forming",
                structural_score,
                invalidation_price,
                anchors,
                regime,
                raw_meta,
                mode,
                render_geometry: None,
                render_style: None,
                render_labels: None,
                // TBM (Three Bar Motion) is candle-only; no pivot-tree
                // dependency, so no level tag.
                pivot_level: None,
            };
            repo.insert(row).await?;
            stats.inserted += 1;
        }
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

/// Merge freshly-computed TBM raw_meta into the existing raw_meta of
/// an open row, preserving lifecycle-enriched fields
/// (bos_bar_index / followthrough_bar_index / retest_* / wyckoff_events
/// / reversal_checklist bos+retest flags) that only `confirm_forming_or_timeout`
/// and `detect_retest_for_confirmed` can produce. Without this merge,
/// every in-place upsert would blow those fields away on the next tick.
fn merge_tbm_raw_meta(
    existing: &serde_json::Value,
    incoming: &serde_json::Value,
) -> serde_json::Value {
    // Preserve keys set downstream by the confirmation / retest passes.
    const PRESERVE: &[&str] = &[
        "bos_bar_index",
        "followthrough_bar_index",
        "confirm_reason",
        "retest_bar_index",
        "retest_time",
        "retest_price",
        "retest_broken_level",
        "wyckoff_events",
    ];
    let mut out = incoming.clone();
    let (Some(out_obj), Some(ex_obj)) = (out.as_object_mut(), existing.as_object()) else {
        return out;
    };
    for key in PRESERVE {
        if let Some(v) = ex_obj.get(*key) {
            out_obj.insert((*key).to_string(), v.clone());
        }
    }
    // Reversal checklist flags `bos` and `retest` are stamped by the
    // lifecycle passes; keep them if they were ever flipped true.
    if let (Some(ex_cl), Some(new_cl)) = (
        ex_obj.get("reversal_checklist"),
        out_obj
            .get_mut("reversal_checklist")
            .and_then(|v| v.as_object_mut()),
    ) {
        if let Some(ex_flags) = ex_cl.get("flags").and_then(|f| f.as_object()) {
            if let Some(new_flags) = new_cl
                .get_mut("flags")
                .and_then(|f| f.as_object_mut())
            {
                for key in ["bos", "retest"] {
                    if let Some(val) = ex_flags.get(key) {
                        if val.as_bool().unwrap_or(false) {
                            new_flags.insert(key.to_string(), val.clone());
                        }
                    }
                }
            }
        }
    }
    out
}

/// P23a+b — walk open forming rows, promote to `confirmed` if BoS +
/// follow-through have printed, or invalidate with reason=timeout if
/// the confirmation window has passed without a break.
async fn confirm_forming_or_timeout(
    repo: &V2DetectionRepository,
    sym: &EngineSymbolRow,
    chronological: &[qtss_storage::MarketBarRow],
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    cfg: &TbmConfirmTuning,
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

    let n = chronological.len();
    if n < 20 { return Ok(()); }
    let latest_idx = n - 1;
    // Reusable ATR(14) at the last bar — BoS + follow-through is
    // measured against *current* volatility, not a stale anchor-era
    // reading, so we can share one value across rows.
    let atr_v = qtss_indicators::volatility::atr(highs, lows, closes, 14);
    let atr_last = atr_v.get(latest_idx).copied().unwrap_or(0.0);
    if !atr_last.is_finite() || atr_last <= 0.0 { return Ok(()); }

    for row in rows {
        let anchor_time: Option<chrono::DateTime<Utc>> = row
            .anchors
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.get("time"))
            .and_then(|t| t.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let Some(anchor_time) = anchor_time else { continue; };
        let Some(anchor_idx) = chronological.iter().position(|b| b.open_time == anchor_time)
        else { continue; };
        if anchor_idx + 1 >= n { continue; }

        let (confirmed, timeout, bos_idx, ft_idx) = match row.subkind.as_str() {
            "bottom_setup" => evaluate_bottom_confirm(
                anchor_idx, latest_idx, highs, lows, closes, atr_last, cfg,
            ),
            "top_setup" => evaluate_top_confirm(
                anchor_idx, latest_idx, highs, lows, closes, atr_last, cfg,
            ),
            _ => continue,
        };

        if confirmed {
            // structural_score already captures pillar weight; we log
            // BoS + follow-through bar indices into raw_meta so the
            // chart can render them and the validator can reuse them.
            let mut meta: serde_json::Value = row.raw_meta.clone();
            if let Some(obj) = meta.as_object_mut() {
                obj.insert("bos_bar_index".into(), json!(bos_idx));
                obj.insert("followthrough_bar_index".into(), json!(ft_idx));
                obj.insert("confirm_reason".into(), json!("bos+followthrough"));
            }
            // P23d — BoS now printed; flip checklist flag and rescore.
            set_checklist_flag(&mut meta, "bos", true);
            apply_reversal_checklist(&mut meta);
            // P25 — label the classic Wyckoff events (SC/AR/ST/SOS for
            // accumulation bottoms, BC/AR/UT/SOW for distribution tops)
            // and persist them alongside anchors for the chart to render.
            let is_bot = row.subkind.as_str() == "bottom_setup";
            let events = label_wyckoff_events(
                chronological, highs, lows, closes, anchor_idx, bos_idx, is_bot,
            );
            if let Some(obj) = meta.as_object_mut() {
                obj.insert("wyckoff_events".into(), json!(events));
            }
            let confidence = (row.structural_score as f32).clamp(0.0, 1.0);
            let channel_scores = json!({
                "structural": row.structural_score,
                "bos": 1.0,
                "followthrough": 1.0,
            });
            // mark_validated writes state='confirmed' + validated_at.
            repo.mark_validated(row.id, confidence, channel_scores, Utc::now())
                .await?;
            // Persist BoS/FT bar indices into raw_meta.
            let _ = repo.update_projection(row.id, row.structural_score, meta).await;
            debug!(id = %row.id, subkind = %row.subkind, bos_idx, ft_idx, "P23: tbm confirmed");
            // P23e — opposite-direction invalidation. A confirmed bottom
            // means any open top_setup on the same symbol/TF is stale
            // by definition (price just proved the reversal the other
            // way), and vice-versa. Without this, stale opposite rows
            // linger in `forming` and pollute the GUI + risk allocator.
            let opp = match row.subkind.as_str() {
                "bottom_setup" => "top_setup",
                "top_setup" => "bottom_setup",
                _ => "",
            };
            if !opp.is_empty() {
                let _ = invalidate_opposite_open(&repo, sym, opp).await;
            }
        } else if timeout {
            repo.update_state(row.id, "invalidated").await?;
            debug!(id = %row.id, subkind = %row.subkind, "P23: tbm invalidated (timeout, no BoS)");
        }
    }
    Ok(())
}

/// Returns (confirmed, timeout, bos_idx_or_zero, ft_idx_or_zero).
fn evaluate_bottom_confirm(
    anchor_idx: usize,
    latest_idx: usize,
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    atr_last: f64,
    cfg: &TbmConfirmTuning,
) -> (bool, bool, usize, usize) {
    // BoS target = highest swing high in the window BEFORE the anchor
    // (what the bottom setup needs to break to prove reversal).
    let pre_start = anchor_idx.saturating_sub(50);
    let pre_hi = highs[pre_start..anchor_idx]
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    if !pre_hi.is_finite() {
        return (false, false, 0, 0);
    }
    // Scan post-anchor bars for close > pre_hi (BoS).
    let scan_end = latest_idx.min(anchor_idx + cfg.window_bars);
    let mut bos_idx: Option<usize> = None;
    for i in (anchor_idx + 1)..=scan_end {
        if closes[i] > pre_hi {
            bos_idx = Some(i);
            break;
        }
    }
    let Some(bos) = bos_idx else {
        let timeout = (latest_idx.saturating_sub(anchor_idx)) > cfg.window_bars;
        return (false, timeout, 0, 0);
    };
    // Follow-through: any bar in [bos, bos+ft_bars] whose close is
    // at least atr_mult * ATR above the anchor low.
    let anchor_low = lows[anchor_idx];
    let trigger = anchor_low + cfg.followthrough_atr_mult * atr_last;
    let ft_end = latest_idx.min(bos + cfg.followthrough_bars);
    for i in bos..=ft_end {
        if closes[i] >= trigger {
            return (true, false, bos, i);
        }
    }
    (false, false, bos, 0)
}

/// P23c — scan confirmed TBM rows for a retest bar (HL for bottom,
/// LH for top) and persist it into raw_meta. Idempotent: rows that
/// already have `retest_bar_index` are skipped.
async fn detect_retest_for_confirmed(
    repo: &V2DetectionRepository,
    sym: &EngineSymbolRow,
    chronological: &[qtss_storage::MarketBarRow],
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    cfg: &TbmConfirmTuning,
) -> anyhow::Result<()> {
    use qtss_storage::DetectionFilter;
    let rows = repo
        .list_filtered(DetectionFilter {
            exchange: Some(&sym.exchange),
            symbol: Some(&sym.symbol),
            timeframe: Some(&sym.interval),
            family: Some("tbm"),
            state: Some("confirmed"),
            mode: None,
            limit: 50,
        })
        .await?;
    if rows.is_empty() { return Ok(()); }

    let n = chronological.len();
    if n < 20 { return Ok(()); }
    let latest_idx = n - 1;
    let atr_v = qtss_indicators::volatility::atr(highs, lows, closes, 14);
    let atr_last = atr_v.get(latest_idx).copied().unwrap_or(0.0);
    if !atr_last.is_finite() || atr_last <= 0.0 { return Ok(()); }

    for row in rows {
        // Skip rows that already carry a retest entry.
        if row
            .raw_meta
            .as_object()
            .map(|m| m.contains_key("retest_bar_index"))
            .unwrap_or(false)
        {
            continue;
        }

        // Locate the anchor + BoS bars from raw_meta.
        let anchor_time: Option<chrono::DateTime<Utc>> = row
            .anchors
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.get("time"))
            .and_then(|t| t.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let Some(anchor_time) = anchor_time else { continue; };
        let Some(anchor_idx) = chronological.iter().position(|b| b.open_time == anchor_time)
        else { continue; };

        let bos_idx: Option<usize> = row
            .raw_meta
            .get("bos_bar_index")
            .and_then(|v| v.as_u64())
            .map(|x| x as usize);
        let Some(bos_idx) = bos_idx else { continue; };
        if bos_idx >= n { continue; }

        // Structural level that was broken.
        let pre_start = anchor_idx.saturating_sub(50);
        let broken_level = match row.subkind.as_str() {
            "bottom_setup" => highs[pre_start..anchor_idx]
                .iter()
                .cloned()
                .fold(f64::NEG_INFINITY, f64::max),
            "top_setup" => lows[pre_start..anchor_idx]
                .iter()
                .cloned()
                .fold(f64::INFINITY, f64::min),
            _ => continue,
        };
        if !broken_level.is_finite() { continue; }

        let tol = cfg.retest_proximity_atr * atr_last;
        let scan_end = latest_idx.min(bos_idx + cfg.retest_max_age_bars);
        let mut retest_idx: Option<usize> = None;

        for i in (bos_idx + 1)..=scan_end {
            match row.subkind.as_str() {
                "bottom_setup" => {
                    // Pullback touches near broken_level from above;
                    // closes back above it → HL retest.
                    let near = (lows[i] - broken_level).abs() <= tol
                        || lows[i] <= broken_level + tol;
                    if near && closes[i] > broken_level && lows[i] > lows[anchor_idx] {
                        retest_idx = Some(i);
                        break;
                    }
                }
                "top_setup" => {
                    // Pullback pokes up near broken_level from below;
                    // closes back below it → LH retest.
                    let near = (highs[i] - broken_level).abs() <= tol
                        || highs[i] >= broken_level - tol;
                    if near && closes[i] < broken_level && highs[i] < highs[anchor_idx] {
                        retest_idx = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }

        let Some(ri) = retest_idx else { continue; };
        let retest_bar = &chronological[ri];
        let retest_price = match row.subkind.as_str() {
            "bottom_setup" => lows[ri],
            "top_setup" => highs[ri],
            _ => continue,
        };

        let mut meta: serde_json::Value = row.raw_meta.clone();
        if let Some(obj) = meta.as_object_mut() {
            obj.insert("retest_bar_index".into(), json!(ri));
            obj.insert("retest_time".into(), json!(retest_bar.open_time.to_rfc3339()));
            obj.insert("retest_price".into(), json!(retest_price));
            obj.insert("retest_broken_level".into(), json!(broken_level));
        }
        // P23d — retest printed; flip checklist flag and rescore.
        set_checklist_flag(&mut meta, "retest", true);
        apply_reversal_checklist(&mut meta);
        // P23g — textbook pullback printed → the row is now at the
        // sniper-entry moment. Promote state confirmed → entry_ready
        // so downstream filters (risk allocator, alerting, GUI) can
        // distinguish "confirmed setup" vs "entry-ready right now".
        let _ = repo.update_state(row.id, "entry_ready").await;
        let _ = repo.update_projection(row.id, row.structural_score, meta).await;
        debug!(
            id = %row.id,
            subkind = %row.subkind,
            retest_bar = ri,
            "P23c: tbm retest detected"
        );
    }
    Ok(())
}

fn evaluate_top_confirm(
    anchor_idx: usize,
    latest_idx: usize,
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    atr_last: f64,
    cfg: &TbmConfirmTuning,
) -> (bool, bool, usize, usize) {
    let pre_start = anchor_idx.saturating_sub(50);
    let pre_lo = lows[pre_start..anchor_idx]
        .iter()
        .cloned()
        .fold(f64::INFINITY, f64::min);
    if !pre_lo.is_finite() {
        return (false, false, 0, 0);
    }
    let scan_end = latest_idx.min(anchor_idx + cfg.window_bars);
    let mut bos_idx: Option<usize> = None;
    for i in (anchor_idx + 1)..=scan_end {
        if closes[i] < pre_lo {
            bos_idx = Some(i);
            break;
        }
    }
    let Some(bos) = bos_idx else {
        let timeout = (latest_idx.saturating_sub(anchor_idx)) > cfg.window_bars;
        return (false, timeout, 0, 0);
    };
    let anchor_high = highs[anchor_idx];
    let trigger = anchor_high - cfg.followthrough_atr_mult * atr_last;
    let ft_end = latest_idx.min(bos + cfg.followthrough_bars);
    for i in bos..=ft_end {
        if closes[i] <= trigger {
            return (true, false, bos, i);
        }
    }
    (false, false, bos, 0)
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

/// P22g — count prior pivot touches that sit within `tol` (fractional,
/// e.g. 0.002 = 0.2%) of the candidate's price. "Prior pivot" = local
/// minimum (for bottom) or maximum (for top) over +/- `r` bars, at an
/// index strictly before the candidate. Bars within `r` of the
/// candidate itself are skipped so adjacent noise doesn't register as
/// a touch. Returns the count (candidate itself NOT counted).
fn count_equal_level_touches(
    lows: &[f64],
    highs: &[f64],
    win_start: usize,
    candidate: usize,
    r: usize,
    tol: f64,
    is_bottom: bool,
) -> usize {
    if candidate <= win_start + r { return 0; }
    let target = if is_bottom { lows[candidate] } else { highs[candidate] };
    if target.abs() < 1e-10 { return 0; }
    let band = target.abs() * tol;
    let mut touches = 0usize;
    // Scan candidate pivots in [win_start + r, candidate - r - 1].
    let scan_end = candidate.saturating_sub(r + 1);
    let mut j = win_start + r;
    while j <= scan_end {
        let l = j.saturating_sub(r);
        let h = (j + r).min(lows.len().saturating_sub(1));
        if is_bottom {
            let local_min = lows[l..=h].iter().cloned().fold(f64::INFINITY, f64::min);
            if (lows[j] - local_min).abs() < 1e-9 && (lows[j] - target).abs() <= band {
                touches += 1;
                j += r + 1; // skip ahead to avoid counting same pivot twice
                continue;
            }
        } else {
            let local_max = highs[l..=h].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            if (highs[j] - local_max).abs() < 1e-9 && (highs[j] - target).abs() <= band {
                touches += 1;
                j += r + 1;
                continue;
            }
        }
        j += 1;
    }
    touches
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
        // Hard gate when `sweep_required` is on — pure-Wyckoff mode.
        if cfg.sweep_required && sweep < 0.5 { continue; }

        // P22g — equal-level (double/triple bottom|top) detection.
        // Count prior pivot lows/highs within `equal_level_tol` of the
        // candidate price. Gives a bonus when ≥ min_touches, optional
        // hard gate via `equal_level_required`.
        let touches = count_equal_level_touches(
            lows, highs, win_start, i, cfg.pivot_radius, cfg.equal_level_tol, is_bottom,
        );
        if cfg.equal_level_required && touches < cfg.equal_level_min_touches { continue; }
        let equal_level = if touches >= cfg.equal_level_min_touches {
            (touches as f64).min(3.0) / 3.0
        } else {
            0.0
        };

        // Composite. Depth is the dominant term so we still prefer the
        // actual window extreme when multiple pivots qualify; wick and
        // volume break ties and demote low-quality pivots.
        let score = depth * 2.0
            + wick_ratio * 1.0
            + vol_term * 1.0
            + sweep * 0.75
            + equal_level * 0.75;
        if score > best_score {
            best_score = score;
            best_idx = Some(i);
        }
    }

    best_idx.unwrap_or(fallback)
}

/// P27 — simple symmetric-radius pivot detector. Returns
/// (high_pivots, low_pivots) as (index, value) pairs where each pivot
/// index dominates the `2*r+1` bar window around it. Skips non-finite
/// samples so NaN-prefix indicator series (RSI/MACD warmup) don't blow
/// up the detector. Used for momentum-divergence pivot sets.
fn compute_pivots(series: &[f64], r: usize) -> (Vec<(usize, f64)>, Vec<(usize, f64)>) {
    let n = series.len();
    if n < 2 * r + 1 { return (Vec::new(), Vec::new()); }
    let mut highs = Vec::new();
    let mut lows = Vec::new();
    for i in r..(n - r) {
        let v = series[i];
        if !v.is_finite() { continue; }
        let mut is_hi = true;
        let mut is_lo = true;
        for j in (i - r)..=(i + r) {
            if j == i { continue; }
            let w = series[j];
            if !w.is_finite() { is_hi = false; is_lo = false; break; }
            if w > v { is_hi = false; }
            if w < v { is_lo = false; }
        }
        if is_hi { highs.push((i, v)); }
        if is_lo { lows.push((i, v)); }
    }
    (highs, lows)
}

/// P23e — invalidate any open forming rows of the opposite subkind on
/// the same exchange+symbol+timeframe. Called right after a confirm so
/// stale counter-direction setups don't survive a proven reversal.
async fn invalidate_opposite_open(
    repo: &V2DetectionRepository,
    sym: &EngineSymbolRow,
    opposite_subkind: &str,
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
    for row in rows.into_iter().filter(|r| r.subkind == opposite_subkind) {
        let _ = repo.update_state(row.id, "invalidated").await;
        debug!(
            id = %row.id,
            subkind = %row.subkind,
            "P23e: opposite-direction invalidation (rival confirmed)"
        );
    }
    Ok(())
}

/// P26 — parse the CSV "ltf:htf,ltf:htf,..." config value into a
/// parent-TF lookup map. Silently drops malformed entries so a bad
/// operator edit never kills the worker.
fn parse_htf_parents(csv: &str) -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    for pair in csv.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Some((k, v)) = pair.split_once(':') {
            let (k, v) = (k.trim(), v.trim());
            if !k.is_empty() && !v.is_empty() {
                m.insert(k.to_string(), v.to_string());
            }
        }
    }
    m
}

/// P26 — look up the most recent forming/confirmed TBM row on the HTF
/// parent for the same symbol + direction. Returns a JSON annotation
/// `{parent_tf, state, detection_id, tier, score}` when present.
async fn fetch_htf_confluence(
    repo: &V2DetectionRepository,
    sym: &EngineSymbolRow,
    parent_tf: &str,
    subkind: &str,
) -> Option<serde_json::Value> {
    use qtss_storage::DetectionFilter;
    // Try confirmed first (stronger signal), then forming.
    for state in ["confirmed", "forming"] {
        let rows = repo
            .list_filtered(DetectionFilter {
                exchange: Some(&sym.exchange),
                symbol: Some(&sym.symbol),
                timeframe: Some(parent_tf),
                family: Some("tbm"),
                state: Some(state),
                mode: None,
                limit: 5,
            })
            .await
            .ok()?;
        if let Some(row) = rows.into_iter().find(|r| r.subkind == subkind) {
            let tier = row
                .raw_meta
                .get("reversal_checklist")
                .and_then(|c| c.get("tier"))
                .and_then(|t| t.as_str())
                .unwrap_or("unknown")
                .to_string();
            let score = row
                .raw_meta
                .get("tbm_score")
                .and_then(|s| s.as_f64())
                .unwrap_or(row.structural_score as f64 * 100.0);
            return Some(json!({
                "parent_tf": parent_tf,
                "state": state,
                "detection_id": row.id.to_string(),
                "tier": tier,
                "score": score,
            }));
        }
    }
    None
}

/// P25 — label the canonical Wyckoff reversal events for a confirmed
/// TBM setup. Bottom / accumulation:
///   SC  (Selling Climax)      = the anchor bar itself (climactic low)
///   AR  (Automatic Rally)     = highest high between anchor+1 .. bos
///   ST  (Secondary Test)      = later pivot low near SC price, lighter
///   SOS (Sign of Strength)    = the BoS bar itself
/// Top / distribution mirrors these: BC / AR / UT / SOW.
///
/// Events are schema-stable: `{label, bar_index, time, price}` — no
/// scoring impact (yet); purely annotative so the GUI can render them
/// and downstream analytics can group by phase.
fn label_wyckoff_events(
    bars: &[qtss_storage::MarketBarRow],
    highs: &[f64],
    lows: &[f64],
    _closes: &[f64],
    anchor_idx: usize,
    bos_idx: usize,
    is_bottom: bool,
) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    if bos_idx <= anchor_idx || bos_idx >= bars.len() {
        return out;
    }
    let ev = |label: &str, idx: usize, price: f64| -> serde_json::Value {
        json!({
            "label": label,
            "bar_index": idx,
            "time": bars[idx].open_time.to_rfc3339(),
            "price": price,
        })
    };

    if is_bottom {
        // SC — anchor low
        out.push(ev("SC", anchor_idx, lows[anchor_idx]));
        // AR — max high in (anchor, bos]
        let mut ar_idx = anchor_idx + 1;
        let mut ar_hi = highs[ar_idx];
        for i in (anchor_idx + 1)..=bos_idx {
            if highs[i] > ar_hi { ar_hi = highs[i]; ar_idx = i; }
        }
        out.push(ev("AR", ar_idx, ar_hi));
        // ST — pivot low in [AR, bos) with low within 1.5% of SC low
        let sc_low = lows[anchor_idx];
        let tol = (sc_low.abs() * 0.015).max(1e-9);
        let mut st_idx: Option<usize> = None;
        for i in (ar_idx + 1)..bos_idx {
            let is_pivot_lo = i > 0 && i + 1 < bars.len()
                && lows[i] <= lows[i - 1] && lows[i] <= lows[i + 1];
            if is_pivot_lo && (lows[i] - sc_low).abs() <= tol && lows[i] > sc_low {
                st_idx = Some(i);
                break;
            }
        }
        if let Some(i) = st_idx {
            out.push(ev("ST", i, lows[i]));
        }
        // SOS — the BoS bar itself
        out.push(ev("SOS", bos_idx, highs[bos_idx]));
    } else {
        out.push(ev("BC", anchor_idx, highs[anchor_idx]));
        let mut ar_idx = anchor_idx + 1;
        let mut ar_lo = lows[ar_idx];
        for i in (anchor_idx + 1)..=bos_idx {
            if lows[i] < ar_lo { ar_lo = lows[i]; ar_idx = i; }
        }
        out.push(ev("AR", ar_idx, ar_lo));
        let bc_hi = highs[anchor_idx];
        let tol = (bc_hi.abs() * 0.015).max(1e-9);
        let mut ut_idx: Option<usize> = None;
        for i in (ar_idx + 1)..bos_idx {
            let is_pivot_hi = i > 0 && i + 1 < bars.len()
                && highs[i] >= highs[i - 1] && highs[i] >= highs[i + 1];
            if is_pivot_hi && (highs[i] - bc_hi).abs() <= tol && highs[i] < bc_hi {
                ut_idx = Some(i);
                break;
            }
        }
        if let Some(i) = ut_idx {
            out.push(ev("UT", i, highs[i]));
        }
        out.push(ev("SOW", bos_idx, lows[bos_idx]));
    }
    out
}

/// P24 — fold the effort-vs-result bonus into a TbmScore's volume
/// pillar and rebalance the weighted total. Details are appended so
/// they flow out through pillar_meta into raw_meta.details.
fn apply_effort_result(
    score: &mut qtss_tbm::TbmScore,
    anchor: usize,
    opens: &[f64],
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    vols: &[f64],
    is_bottom: bool,
    cfg: &qtss_tbm::TbmEffortResultTuning,
) {
    let lo = anchor.saturating_sub(20);
    if anchor < lo || anchor >= opens.len() { return; }
    let (pts, details) = qtss_tbm::volume::score_effort_result(
        &opens[lo..=anchor],
        &highs[lo..=anchor],
        &lows[lo..=anchor],
        &closes[lo..=anchor],
        &vols[lo..=anchor],
        is_bottom,
        cfg,
    );
    if pts <= 0.0 { return; }
    for p in score.pillars.iter_mut() {
        if matches!(p.kind, qtss_tbm::PillarKind::Volume) {
            p.score = (p.score + pts).min(100.0);
            p.details.extend(details.iter().cloned());
        }
    }
    // Recompute weighted total with the patched volume score.
    let mut total_w = 0.0;
    let mut weighted = 0.0;
    for p in &score.pillars {
        let w = p.weight.max(0.0);
        total_w += w;
        weighted += p.score * w;
    }
    if total_w > 0.0 {
        score.total = (weighted / total_w).min(100.0);
    }
}

/// P23d — evaluate the three anchor-bar checklist flags at a chosen
/// index (re-deriving the same logic `pick_anchor` uses internally so
/// the picker can stay a pure usize return). Returns (sweep, rejection,
/// volume). `rejection` = wick ≥ cfg.wick_min_ratio; `volume` = bar
/// volume ≥ cfg.vol_min_ratio × 20-bar avg; `sweep` = bar takes out a
/// prior window extreme.
fn anchor_flags(
    opens: &[f64],
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    vols: &[f64],
    win_start: usize,
    n: usize,
    idx: usize,
    is_bottom: bool,
    cfg: &TbmAnchorTuning,
) -> (bool, bool, bool) {
    if idx >= n || idx < win_start {
        return (false, false, false);
    }
    let range_i = (highs[idx] - lows[idx]).max(1e-10);
    let body_hi = opens[idx].max(closes[idx]);
    let body_lo = opens[idx].min(closes[idx]);
    let wick_ratio = if is_bottom {
        ((body_lo - lows[idx]).max(0.0)) / range_i
    } else {
        ((highs[idx] - body_hi).max(0.0)) / range_i
    };
    let rejection = wick_ratio >= cfg.wick_min_ratio;

    let v_start = idx.saturating_sub(20).max(win_start);
    let v_count = (idx - v_start).max(1) as f64;
    let v_avg = vols[v_start..idx].iter().sum::<f64>() / v_count;
    let v_ratio = if v_avg > 0.0 { vols[idx] / v_avg } else { 0.0 };
    let volume_ok = v_ratio >= cfg.vol_min_ratio;

    let sweep = if is_bottom {
        let prior_lo = lows[win_start..idx].iter().cloned().fold(f64::INFINITY, f64::min);
        prior_lo.is_finite() && lows[idx] < prior_lo * 0.9995
    } else {
        let prior_hi = highs[win_start..idx].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        prior_hi.is_finite() && highs[idx] > prior_hi * 1.0005
    };

    (sweep, rejection, volume_ok)
}

/// P23d — flip one flag inside `raw_meta.reversal_checklist.flags`
/// without clobbering neighbours. Creates the subtree if missing so
/// older rows emitted before P23d still aggregate cleanly.
fn set_checklist_flag(meta: &mut serde_json::Value, key: &str, value: bool) {
    let obj = match meta.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    let checklist = obj
        .entry("reversal_checklist")
        .or_insert_with(|| json!({ "flags": {} }));
    let flags = checklist
        .as_object_mut()
        .and_then(|c| {
            c.entry("flags").or_insert_with(|| json!({}));
            c.get_mut("flags").and_then(|v| v.as_object_mut())
        });
    if let Some(flags) = flags {
        flags.insert(key.into(), json!(value));
    }
}

/// P23d — recompute `reversal_checklist.score` (0–5) + `tier` from the
/// five boolean flags. Tier ladder:
///   5 → elite, 4 → strong, 3 → ok, 2 → weak, ≤1 → filtered.
/// Downstream filters (GUI, alerting, risk allocator) key off `tier` so
/// the scorecard stays a single source of truth.
fn apply_reversal_checklist(meta: &mut serde_json::Value) {
    let Some(obj) = meta.as_object_mut() else { return };
    let checklist = obj
        .entry("reversal_checklist")
        .or_insert_with(|| json!({ "flags": {} }));
    let Some(cl_obj) = checklist.as_object_mut() else { return };
    let flags_val = cl_obj.entry("flags").or_insert_with(|| json!({}));
    let flags = flags_val.as_object().cloned().unwrap_or_default();
    let get = |k: &str| flags.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let components = ["sweep", "rejection", "bos", "retest", "volume"];
    let score: u32 = components.iter().map(|k| if get(k) { 1 } else { 0 }).sum();
    let tier = match score {
        5 => "elite",
        4 => "strong",
        3 => "ok",
        2 => "weak",
        _ => "filtered",
    };
    cl_obj.insert("score".into(), json!(score));
    cl_obj.insert("tier".into(), json!(tier));
}

/// Map a Binance kline interval string (e.g. "15m", "1h", "4h") to
/// seconds. Mirrors ;
/// duplicated here instead of re-exporting so the two loops stay
/// decoupled. Returns None for unknown/typo values so the caller can
/// fall back to legacy semantics.
fn interval_to_seconds(iv: &str) -> Option<u64> {
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
