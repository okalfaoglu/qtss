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
    config::{TbmConfig, TbmMtfTuning, TbmPillarWeights, TbmSetupTuning},
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

    TbmConfig {
        enabled,
        tick_interval_s,
        lookback_bars,
        weights,
        setup,
        mtf,
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

    // ----- Indicator inputs ------------------------------------------
    let stoch = stochastic(&highs, &lows, &closes, 14, 3);
    let macd_r = macd(&closes, 12, 26, 9);
    let bb = bollinger(&closes, 20, 2.0);
    let mfi_v = mfi(&highs, &lows, &closes, &vols, 14);
    let obv_v = obv(&closes, &vols);
    let cvd_v = cvd(&highs, &lows, &closes, &vols);
    let _atr_v = atr(&highs, &lows, &closes, 14);
    let ema_fast = ema(&closes, 9);
    let ema_slow = ema(&closes, 21);

    let stoch_k = finite_or(stoch.k.get(last).copied(), 50.0);
    let stoch_d = finite_or(stoch.d.get(last).copied(), 50.0);
    let macd_hist = finite_or(macd_r.histogram.get(last).copied(), 0.0);
    let macd_hist_prev = finite_or(macd_r.histogram.get(last.saturating_sub(1)).copied(), 0.0);
    let ema_fast_last = finite_or(ema_fast.get(last).copied(), closes[last]);
    let ema_slow_last = finite_or(ema_slow.get(last).copied(), closes[last]);
    let bb_pct_b = finite_or(bb.percent_b.get(last).copied(), 0.5);
    let bb_squeeze = bb
        .bandwidth
        .get(last)
        .copied()
        .map(|w| w.is_finite() && w < 0.05)
        .unwrap_or(false);
    let mfi_last = finite_or(mfi_v.get(last).copied(), 50.0);
    let obv_slope = slope_last_n(&obv_v, 20);
    // CVD slope: bar-delta CVD'nin son 20 bar eğimi. Sıfır olarak
    // hardcode'luydu — volume pillar'ın CVD ayağı hiç puan alamıyor,
    // total skor her sembolde ~30'a kilitleniyordu. Canlı akışa bağlı
    // trade-flow CVD'si gelene kadar bar-bazlı tahmin doğru yönde
    // sinyal veriyor.
    let cvd_slope = slope_last_n(&cvd_v, 20);
    let vol_last = vols[last];
    let vol_avg = window_mean(&vols, 20);

    // Swing extremes for fib proximity (very simple — last 50 bars).
    let win_start = n.saturating_sub(50);
    let swing_high = highs[win_start..n].iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let swing_low = lows[win_start..n].iter().cloned().fold(f64::INFINITY, f64::min);
    let last_close = closes[last];
    let (fib_proximity, fib_level_name) = nearest_fib(swing_high, swing_low, last_close);

    // Divergence pivots — keep empty for now; v1 fills these from a
    // dedicated pivot scanner. The momentum pillar handles empty inputs
    // gracefully (returns 0 for the divergence sub-score).
    let empty: Vec<(usize, f64)> = Vec::new();

    // ----- Pillar evaluation (bottom + top) --------------------------
    let bottom = build_score(
        cfg,
        true,
        stoch_k, stoch_d, macd_hist, macd_hist_prev,
        ema_fast_last, ema_slow_last,
        &empty, &empty, &empty, &empty,
        mfi_last, obv_slope, cvd_slope, vol_last, vol_avg,
        fib_proximity, fib_level_name, bb_pct_b, bb_squeeze,
        onchain_metrics.as_ref(),
    );
    let top = build_score(
        cfg,
        false,
        stoch_k, stoch_d, macd_hist, macd_hist_prev,
        ema_fast_last, ema_slow_last,
        &empty, &empty, &empty, &empty,
        mfi_last, obv_slope, cvd_slope, vol_last, vol_avg,
        fib_proximity, fib_level_name, bb_pct_b, bb_squeeze,
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
    let last_bar_time = chronological[last].open_time;
    let invalidation = invalidation_for(swing_high, swing_low, last_close);

    for setup in &setups {
        stats.emitted += 1;
        if dedup_open(&repo, sym, setup, last_bar_time).await? {
            continue;
        }

        let subkind = subkind_for(setup.direction);

        // P21 — anchor the label to the STRUCTURAL extremum in the
        // lookback window, not the latest bar. A `bottom_setup` drawn
        // on the last bar while price is mid-spike looks like the
        // label is pinned to the wrong bar (user report: "bottom_setup
        // 42% Weak" floating on top of a rally mum). The structural
        // low/high inside the same 50-bar window that feeds swing_high/
        // swing_low is the correct anchor.
        let (anchor_idx, anchor_px_f64) = match setup.direction {
            SetupDirection::Bottom => {
                let slice = &lows[win_start..n];
                let (rel_i, &px) = slice
                    .iter()
                    .enumerate()
                    .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(i, v)| (i, v))
                    .unwrap_or((slice.len().saturating_sub(1), &last_close));
                (win_start + rel_i, px)
            }
            SetupDirection::Top => {
                let slice = &highs[win_start..n];
                let (rel_i, &px) = slice
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(i, v)| (i, v))
                    .unwrap_or((slice.len().saturating_sub(1), &last_close));
                (win_start + rel_i, px)
            }
        };
        let anchor_bar_time = chronological[anchor_idx].open_time;
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
    obv_slope: f64,
    cvd_slope: f64,
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

    let mut volume = score_volume(mfi_last, obv_slope, cvd_slope, vol_last, vol_avg, is_bottom);
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

async fn dedup_open(
    repo: &V2DetectionRepository,
    sym: &EngineSymbolRow,
    setup: &TbmSetup,
    last_bar_time: chrono::DateTime<Utc>,
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
            limit: 5,
        })
        .await?;
    let target_subkind = subkind_for(setup.direction);
    for row in rows {
        if row.subkind == target_subkind && row.detected_at >= last_bar_time {
            return Ok(true);
        }
    }
    Ok(false)
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
