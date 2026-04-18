//! v2 detector orchestrator — Faz 7 Adım 2.
//!
//! Periodic loop that turns the dormant detector crates (Elliott,
//! Harmonic, Classical, Wyckoff) into a live pipeline that writes rows
//! into `qtss_v2_detections`. The chart endpoint and Detections panel
//! both read from that table.
//!
//! ## Design
//!
//! * **Pure dispatch table** (CLAUDE.md #1): every detector implements
//!   the [`DetectorRunner`] trait. Adding a new family means appending
//!   one entry to [`build_runners`] — no scattered `if/else` over
//!   detector kinds.
//! * **Config-driven** (CLAUDE.md #2): every toggle, threshold, and
//!   poll interval is read from `system_config` via the `resolve_*`
//!   helpers. The hardcoded defaults in this file are *bootstrap*
//!   defaults — fall-throughs for fresh deployments — not source-of-truth
//!   tunables.
//! * **Stateless across ticks**: each pass rebuilds the pivot + regime
//!   engines from the most recent `history_bars` bars in `market_bars`.
//!   This is intentional: a streaming/event-driven version arrives in
//!   Adım 3+ once the EventBus wiring lands. Until then "rebuild from
//!   history" is the simplest correct thing.
//! * **Dedup**: before inserting a detection we look up the most recent
//!   open row for the same (exchange, symbol, tf, family, subkind) and
//!   compare the last anchor's `bar_index`. Identical → skip. This
//!   prevents the orchestrator from spamming the table when the same
//!   impulse persists across many ticks.
//!
//! Gated entirely by `detection.orchestrator.enabled` so the loop is a
//! no-op until an operator flips it on from the Config panel.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures_util::stream::{self, StreamExt};

use chrono::Utc;
use qtss_chart_patterns::{analyze_trading_range, OhlcBar, TradingRangeParams};
use qtss_classical::{ClassicalConfig, ClassicalDetector};
use qtss_candles::{CandleConfig, CandleDetector};
use qtss_gap::{GapConfig, GapDetector};
use qtss_range::RangeDetectorConfig;
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::detection::{Detection, PatternKind, PatternState, PivotRef};
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::pivot::{PivotKind, PivotLevel, PivotTree};
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;
use qtss_elliott::{ElliottConfig, ElliottDetectorSet, ElliottFormationToggles};
use qtss_harmonic::{HarmonicConfig, HarmonicDetector};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use qtss_pivots::{PivotConfig, PivotEngine};
use qtss_regime::{RegimeConfig, RegimeEngine};
use qtss_storage::{
    list_enabled_engine_symbols, list_recent_bars, resolve_system_f64, resolve_system_string,
    resolve_system_u64, resolve_worker_enabled_flag, DetectionFilter, EngineSymbolRow,
    MarketBarRow, NewDetection, V2DetectionRepository,
    max_cached_bar_index, upsert_pivot_cache_batch, PivotCacheRow,
};
use qtss_wyckoff::{WyckoffConfig, WyckoffDetector};
use serde_json::json;
use sqlx::PgPool;
use tracing::{debug, info, warn};
use uuid::Uuid;

// ---------------------------------------------------------------------
// Trait + dispatch table
// ---------------------------------------------------------------------

/// Polymorphic detector. Each family has one impl that delegates to
/// the underlying crate. The orchestrator iterates a `Vec<Box<dyn>>`
/// so adding/removing families never touches the loop body.
/// Polymorphic detector. Each family has one impl that delegates to
/// the underlying crate. The orchestrator iterates a `Vec<Box<dyn>>`
/// so adding/removing families never touches the loop body.
///
/// `bars` carries the same chronological window the orchestrator fed
/// into the pivot/regime engines. Tree-only detectors (Elliott,
/// Harmonic, Classical, Wyckoff) ignore it; bar-driven detectors
/// (Range / future TBM-shaped families) read from it directly.
pub(crate) trait DetectorRunner: Send + Sync {
    /// Stable family key: "elliott" / "harmonic" / "classical" / "wyckoff" / "range".
    /// Reserved for richer dispatch / metrics in Adım 3 (validator wiring).
    #[allow(dead_code)]
    fn family(&self) -> &'static str;

    fn detect(
        &self,
        tree: &PivotTree,
        bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection>;
}


struct ElliottRunner(ElliottDetectorSet);
impl DetectorRunner for ElliottRunner {
    fn family(&self) -> &'static str {
        "elliott"
    }
    fn detect(
        &self,
        tree: &PivotTree,
        _bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        self.0.detect_all(tree, instrument, timeframe, regime)
    }
}

struct HarmonicRunner(HarmonicDetector);
impl DetectorRunner for HarmonicRunner {
    fn family(&self) -> &'static str {
        "harmonic"
    }
    fn detect(
        &self,
        tree: &PivotTree,
        _bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        self.0.detect(tree, instrument, timeframe, regime).into_iter().collect()
    }
}

struct ClassicalRunner(ClassicalDetector);
impl DetectorRunner for ClassicalRunner {
    fn family(&self) -> &'static str {
        "classical"
    }
    fn detect(
        &self,
        tree: &PivotTree,
        bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        // P5.2 — bars plumbed through so Flag / Pennant flagpole ATR
        // checks have bar-level OHLC; bar-less shapes ignore the slice.
        self.0
            .detect_with_bars(tree, bars, instrument, timeframe, regime)
            .into_iter()
            .collect()
    }
}

/// Gap runner — wraps `qtss_gap::GapDetector`. Consumes bars only
/// (gap detection is price/volume based, no pivot tree required).
struct GapRunner(GapDetector);
impl DetectorRunner for GapRunner {
    fn family(&self) -> &'static str {
        "gap"
    }
    fn detect(
        &self,
        _tree: &PivotTree,
        bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        self.0
            .detect(bars, instrument, timeframe, regime)
            .into_iter()
            .collect()
    }
}

/// Candle runner — wraps `qtss_candles::CandleDetector`. Pure bar-based;
/// pivot tree is not consulted (candlestick classification is per-bar
/// geometry + short trend context).
struct CandleRunner(CandleDetector);
impl DetectorRunner for CandleRunner {
    fn family(&self) -> &'static str {
        "candle"
    }
    fn detect(
        &self,
        _tree: &PivotTree,
        bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        self.0
            .detect(bars, instrument, timeframe, regime)
            .into_iter()
            .collect()
    }
}

struct WyckoffRunner(WyckoffDetector);
impl DetectorRunner for WyckoffRunner {
    fn family(&self) -> &'static str {
        "wyckoff"
    }
    fn detect(
        &self,
        tree: &PivotTree,
        bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        // P1 refactor: bar-level OHLC plumbed through so bar-aware event
        // detectors (SOS/SOW shape, Markup/Markdown, JAC body ratio …)
        // can see the full candle anatomy, not just pivots.
        self.0.detect_with_bars(tree, bars, instrument, timeframe, regime).into_iter().collect()
    }
}

/// Range runner — wraps `qtss_chart_patterns::trading_range::analyze_trading_range`
/// in the same `DetectorRunner` shape as the structural detectors so
/// trading-range setups land in `qtss_v2_detections` alongside Elliott
/// & friends. Pure mapping: `setup_score_best/100` → `structural_score`,
/// `setup_side` → `subkind`, range_high/low → two anchors,
/// opposite band → `invalidation_price`, full `TradingRangeResult` →
/// `raw_meta`.
///
/// State machine semantics (point-in-time, matches the rest of the
/// orchestrator — durum-style transitions stay in qtss-analysis):
///   - guardrails_pass + setup_side != "NOTR" → Confirmed
///   - is_range_regime                       → Forming
///   - otherwise                             → no detection (skip)
struct RangeRunner {
    params: TradingRangeParams,
}

impl RangeRunner {
    fn new(params: TradingRangeParams) -> Self {
        Self { params }
    }
}

impl DetectorRunner for RangeRunner {
    fn family(&self) -> &'static str {
        "range"
    }

    fn detect(
        &self,
        _tree: &PivotTree,
        bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        if bars.len() < 30 {
            return Vec::new();
        }

        // Bar (Decimal) → OhlcBar (f64). bar_index is the chronological
        // window index so anchors line up with what the orchestrator
        // already enriches with open_time.
        let ohlc: Vec<OhlcBar> = bars
            .iter()
            .enumerate()
            .map(|(i, b)| OhlcBar {
                open: b.open.to_string().parse().unwrap_or(0.0),
                high: b.high.to_string().parse().unwrap_or(0.0),
                low: b.low.to_string().parse().unwrap_or(0.0),
                close: b.close.to_string().parse().unwrap_or(0.0),
                bar_index: i as i64,
                volume: b.volume.to_string().parse().ok(),
            })
            .collect();

        let res = analyze_trading_range(&ohlc, &self.params);
        if !res.valid {
            return Vec::new();
        }

        let (Some(range_high), Some(range_low)) = (res.range_high, res.range_low) else {
            return Vec::new();
        };

        let (state, subkind) = match (res.guardrails_pass, res.setup_side.as_str()) {
            (true, "LONG") => (PatternState::Confirmed, "long_setup"),
            (true, "SHORT") => (PatternState::Confirmed, "short_setup"),
            _ if res.is_range_regime => (PatternState::Forming, "range_regime"),
            _ => return Vec::new(),
        };

        let high_decimal = Decimal::from_f64(range_high).unwrap_or_default();
        let low_decimal = Decimal::from_f64(range_low).unwrap_or_default();
        let last_idx = res
            .last_bar_index
            .map(|i| i as u64)
            .unwrap_or_else(|| bars.len().saturating_sub(1) as u64);

        // Two anchors: resistance + support. bar_index points at the
        // last evaluated bar so the chart can pin the band to "now".
        let anchors = vec![
            PivotRef {
                bar_index: last_idx,
                price: high_decimal,
                level: PivotLevel::L0,
                label: Some("resistance".to_string()),
            },
            PivotRef {
                bar_index: last_idx,
                price: low_decimal,
                level: PivotLevel::L0,
                label: Some("support".to_string()),
            },
        ];

        // For a LONG setup the range_high is the breakout invalidation
        // (price closing above means the range broke); SHORT mirrors.
        // Forming/range_regime falls back to the wider band.
        let invalidation_price = match subkind {
            "long_setup" => high_decimal,
            "short_setup" => low_decimal,
            _ => high_decimal,
        };

        let structural_score = (res.setup_score_best as f32 / 100.0).clamp(0.0, 1.0);

        let raw_meta = serde_json::to_value(&res).unwrap_or(serde_json::Value::Null);

        let detection = Detection::new(
            instrument.clone(),
            timeframe,
            PatternKind::Range(subkind.to_string()),
            state,
            anchors,
            structural_score,
            invalidation_price,
            regime.clone(),
        );
        let mut detection = detection;
        detection.raw_meta = raw_meta;
        vec![detection]
    }
}

/// Range sub-detector runner — FVG, Order Block, Liquidity Pool, Equal Highs/Lows.
/// Wraps `qtss_range::detect_all` into DetectorRunner. Each sub-detection
/// becomes a separate `Detection` with `PatternKind::Range(subkind)`.
struct RangeSubRunner {
    cfg: RangeDetectorConfig,
}

impl RangeSubRunner {
    fn new(cfg: RangeDetectorConfig) -> Self {
        Self { cfg }
    }
}

impl DetectorRunner for RangeSubRunner {
    fn family(&self) -> &'static str {
        "range"
    }

    fn detect(
        &self,
        _tree: &PivotTree,
        bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        if bars.len() < 30 {
            return Vec::new();
        }

        let ohlc: Vec<qtss_range::OhlcBar> = bars
            .iter()
            .enumerate()
            .map(|(i, b)| qtss_range::OhlcBar {
                open: b.open.to_string().parse().unwrap_or(0.0),
                high: b.high.to_string().parse().unwrap_or(0.0),
                low: b.low.to_string().parse().unwrap_or(0.0),
                close: b.close.to_string().parse().unwrap_or(0.0),
                bar_index: i as i64,
                volume: b.volume.to_string().parse().ok(),
            })
            .collect();

        // Compute ATR for the sub-detectors
        let atr_series = qtss_range::helpers::wilder_atr(&ohlc, 14);
        let atr_value = atr_series.last().copied().unwrap_or(0.0);
        if !atr_value.is_finite() || atr_value <= 1e-12 {
            return Vec::new();
        }

        let matches = qtss_range::detect_all(&ohlc, atr_value, &self.cfg);
        let mut detections = Vec::new();

        for m in matches {
            let high_decimal = Decimal::from_f64(m.zone_high).unwrap_or_default();
            let low_decimal = Decimal::from_f64(m.zone_low).unwrap_or_default();
            let bar_idx = m.bar_index as u64;

            let anchors = vec![
                PivotRef {
                    bar_index: bar_idx,
                    price: high_decimal,
                    level: PivotLevel::L0,
                    label: Some(format!("{}_high", m.subkind)),
                },
                PivotRef {
                    bar_index: bar_idx,
                    price: low_decimal,
                    level: PivotLevel::L0,
                    label: Some(format!("{}_low", m.subkind)),
                },
            ];

            // Invalidation: opposite boundary
            let invalidation = if m.subkind.contains("bullish") || m.subkind.contains("_low") || m.subkind.contains("equal_lows") {
                high_decimal
            } else {
                low_decimal
            };

            let structural_score = m.score as f32;

            let mut det = Detection::new(
                instrument.clone(),
                timeframe,
                PatternKind::Range(m.subkind),
                PatternState::Confirmed,
                anchors,
                structural_score.clamp(0.0, 1.0),
                invalidation,
                regime.clone(),
            );
            det.raw_meta = m.meta;
            detections.push(det);
        }

        detections
    }
}

/// Resolve pivot-level string to enum. Falls back to L1 if unknown.
fn parse_pivot_level(s: &str) -> PivotLevel {
    match s.to_lowercase().as_str() {
        "l0" | "0" => PivotLevel::L0,
        "l1" | "1" => PivotLevel::L1,
        "l2" | "2" => PivotLevel::L2,
        "l3" | "3" => PivotLevel::L3,
        _ => PivotLevel::L1,
    }
}

/// Resolve harmonic detector config from system_config (CLAUDE.md #2).
/// Every threshold is tuneable from the Config Editor without restarts.
async fn resolve_harmonic_config(pool: &PgPool) -> HarmonicConfig {
    let level_str = resolve_system_string(
        pool, "detection", "harmonic.pivot_level",
        "QTSS_DETECTION_HARMONIC_PIVOT_LEVEL", "L0",
    ).await;
    let min_score = resolve_system_f64(
        pool, "detection", "harmonic.min_structural_score",
        "QTSS_DETECTION_HARMONIC_MIN_SCORE", 0.40,
    ).await as f32;
    let slack = resolve_system_f64(
        pool, "detection", "harmonic.global_slack",
        "QTSS_DETECTION_HARMONIC_GLOBAL_SLACK", 0.05,
    ).await;

    HarmonicConfig {
        pivot_level: parse_pivot_level(&level_str),
        min_structural_score: min_score,
        global_slack: slack.clamp(0.0, 0.5),
    }
}

/// Resolve classical detector config from system_config (CLAUDE.md #2).
/// Every threshold is tuneable from the Config Editor without restarts.
async fn resolve_gap_config(pool: &PgPool) -> GapConfig {
    let min_gap_pct = resolve_system_f64(
        pool, "detection", "gap.min_gap_pct",
        "QTSS_DETECTION_GAP_MIN_PCT", 0.005,
    ).await;
    let volume_sma_bars = resolve_system_u64(
        pool, "detection", "gap.volume_sma_bars",
        "QTSS_DETECTION_GAP_VOL_SMA", 20, 2, 500,
    ).await as usize;
    let vol_mult_breakaway = resolve_system_f64(
        pool, "detection", "gap.vol_mult_breakaway",
        "QTSS_DETECTION_GAP_VOL_MULT_BREAKAWAY", 1.5,
    ).await;
    let vol_mult_runaway = resolve_system_f64(
        pool, "detection", "gap.vol_mult_runaway",
        "QTSS_DETECTION_GAP_VOL_MULT_RUNAWAY", 1.3,
    ).await;
    let vol_mult_exhaustion = resolve_system_f64(
        pool, "detection", "gap.vol_mult_exhaustion",
        "QTSS_DETECTION_GAP_VOL_MULT_EXHAUSTION", 1.8,
    ).await;
    let range_flat_pct = resolve_system_f64(
        pool, "detection", "gap.range_flat_pct",
        "QTSS_DETECTION_GAP_RANGE_FLAT_PCT", 0.02,
    ).await;
    let consolidation_lookback = resolve_system_u64(
        pool, "detection", "gap.consolidation_lookback",
        "QTSS_DETECTION_GAP_CONSOL_LB", 10, 3, 500,
    ).await as usize;
    let runaway_trend_bars = resolve_system_u64(
        pool, "detection", "gap.runaway_trend_bars",
        "QTSS_DETECTION_GAP_RUNAWAY_BARS", 5, 2, 200,
    ).await as usize;
    let runaway_trend_min_pct = resolve_system_f64(
        pool, "detection", "gap.runaway_trend_min_pct",
        "QTSS_DETECTION_GAP_RUNAWAY_MIN_PCT", 0.02,
    ).await;
    let exhaustion_reversal_bars = resolve_system_u64(
        pool, "detection", "gap.exhaustion_reversal_bars",
        "QTSS_DETECTION_GAP_EXHAUSTION_REV_BARS", 5, 1, 200,
    ).await as usize;
    let island_max_bars = resolve_system_u64(
        pool, "detection", "gap.island_max_bars",
        "QTSS_DETECTION_GAP_ISLAND_MAX_BARS", 10, 1, 200,
    ).await as usize;
    let min_structural_score = resolve_system_f64(
        pool, "detection", "gap.min_structural_score",
        "QTSS_DETECTION_GAP_MIN_SCORE", 0.50,
    ).await as f32;
    GapConfig {
        min_gap_pct,
        volume_sma_bars,
        vol_mult_breakaway,
        vol_mult_runaway,
        vol_mult_exhaustion,
        range_flat_pct,
        consolidation_lookback,
        runaway_trend_bars,
        runaway_trend_min_pct,
        exhaustion_reversal_bars,
        island_max_bars,
        min_structural_score,
    }
}

/// Candle detector config — every threshold tuneable from Config Editor.
async fn resolve_candle_config(pool: &PgPool) -> CandleConfig {
    let doji_body_ratio_max = resolve_system_f64(
        pool, "detection", "candle.doji_body_ratio_max",
        "QTSS_DETECTION_CANDLE_DOJI_BODY_MAX", 0.1,
    ).await;
    let marubozu_shadow_ratio_max = resolve_system_f64(
        pool, "detection", "candle.marubozu_shadow_ratio_max",
        "QTSS_DETECTION_CANDLE_MARUBOZU_SHADOW_MAX", 0.05,
    ).await;
    let hammer_lower_shadow_ratio_min = resolve_system_f64(
        pool, "detection", "candle.hammer_lower_shadow_ratio_min",
        "QTSS_DETECTION_CANDLE_HAMMER_LOWER_MIN", 2.0,
    ).await;
    let hammer_upper_shadow_ratio_max = resolve_system_f64(
        pool, "detection", "candle.hammer_upper_shadow_ratio_max",
        "QTSS_DETECTION_CANDLE_HAMMER_UPPER_MAX", 0.5,
    ).await;
    let spinning_top_body_ratio_max = resolve_system_f64(
        pool, "detection", "candle.spinning_top_body_ratio_max",
        "QTSS_DETECTION_CANDLE_SPINNING_BODY_MAX", 0.3,
    ).await;
    let tweezer_price_tol = resolve_system_f64(
        pool, "detection", "candle.tweezer_price_tol",
        "QTSS_DETECTION_CANDLE_TWEEZER_TOL", 0.002,
    ).await;
    let trend_context_bars = resolve_system_u64(
        pool, "detection", "candle.trend_context_bars",
        "QTSS_DETECTION_CANDLE_TREND_BARS", 5, 2, 200,
    ).await as usize;
    let trend_context_min_pct = resolve_system_f64(
        pool, "detection", "candle.trend_context_min_pct",
        "QTSS_DETECTION_CANDLE_TREND_MIN_PCT", 0.015,
    ).await;
    let min_structural_score = resolve_system_f64(
        pool, "detection", "candle.min_structural_score",
        "QTSS_DETECTION_CANDLE_MIN_SCORE", 0.50,
    ).await as f32;
    let min_timeframe_seconds = resolve_system_u64(
        pool, "detection", "candle.min_timeframe_seconds",
        "QTSS_DETECTION_CANDLE_MIN_TF_SEC", 900, 0, 86_400,
    ).await as i64;
    CandleConfig {
        doji_body_ratio_max,
        marubozu_shadow_ratio_max,
        hammer_lower_shadow_ratio_min,
        hammer_upper_shadow_ratio_max,
        spinning_top_body_ratio_max,
        tweezer_price_tol,
        trend_context_bars,
        trend_context_min_pct,
        min_structural_score,
        min_timeframe_seconds,
    }
}

async fn resolve_classical_config(pool: &PgPool) -> ClassicalConfig {
    let level_str = resolve_system_string(
        pool, "detection", "classical.pivot_level",
        "QTSS_DETECTION_CLASSICAL_PIVOT_LEVEL", "L1",
    ).await;
    let min_score = resolve_system_f64(
        pool, "detection", "classical.min_structural_score",
        "QTSS_DETECTION_CLASSICAL_MIN_SCORE", 0.50,
    ).await as f32;
    let eq_tol = resolve_system_f64(
        pool, "detection", "classical.equality_tolerance",
        "QTSS_DETECTION_CLASSICAL_EQ_TOL", 0.03,
    ).await;
    let apex_h = resolve_system_u64(
        pool, "detection", "classical.apex_horizon_bars",
        "QTSS_DETECTION_CLASSICAL_APEX_HORIZON", 50, 1, 500,
    ).await;
    let flat_pct = resolve_system_f64(
        pool, "detection", "classical.flatness_threshold_pct",
        "QTSS_DETECTION_CLASSICAL_FLATNESS_PCT", 0.001,
    ).await;
    let flat_min = resolve_system_f64(
        pool, "detection", "classical.flatness_min_score",
        "QTSS_DETECTION_CLASSICAL_FLATNESS_MIN", 0.3,
    ).await;
    let neck_mult = resolve_system_f64(
        pool, "detection", "classical.neckline_tolerance_mult",
        "QTSS_DETECTION_CLASSICAL_NECK_MULT", 1.5,
    ).await;
    let tri_sym = resolve_system_f64(
        pool, "detection", "classical.triangle_symmetry_tol",
        "QTSS_DETECTION_CLASSICAL_TRI_SYM", 0.5,
    ).await;
    let hs_slope = resolve_system_f64(
        pool, "detection", "classical.hs_max_neckline_slope_pct",
        "QTSS_DETECTION_CLASSICAL_HS_SLOPE", 0.003,
    ).await;
    let hs_time = resolve_system_f64(
        pool, "detection", "classical.hs_time_symmetry_tol",
        "QTSS_DETECTION_CLASSICAL_HS_TIME_SYM", 0.5,
    ).await;
    let rect_slope = resolve_system_f64(
        pool, "detection", "classical.rectangle_max_slope_pct",
        "QTSS_DETECTION_CLASSICAL_RECT_SLOPE", 0.002,
    ).await;
    let rect_min_bars = resolve_system_u64(
        pool, "detection", "classical.rectangle_min_bars",
        "QTSS_DETECTION_CLASSICAL_RECT_MIN_BARS", 15, 1, 500,
    ).await;
    let flag_pole_atr = resolve_system_f64(
        pool, "detection", "classical.flag_pole_min_move_atr",
        "QTSS_DETECTION_CLASSICAL_FLAG_POLE_ATR", 3.0,
    ).await;
    let flag_pole_bars = resolve_system_u64(
        pool, "detection", "classical.flag_pole_max_bars",
        "QTSS_DETECTION_CLASSICAL_FLAG_POLE_BARS", 20, 1, 500,
    ).await;
    let flag_retrace = resolve_system_f64(
        pool, "detection", "classical.flag_max_retrace_pct",
        "QTSS_DETECTION_CLASSICAL_FLAG_RETRACE", 0.5,
    ).await;
    let flag_atr_period = resolve_system_u64(
        pool, "detection", "classical.flag_atr_period",
        "QTSS_DETECTION_CLASSICAL_FLAG_ATR_PERIOD", 14, 1, 500,
    ).await;
    let flag_parallel = resolve_system_f64(
        pool, "detection", "classical.flag_parallelism_tol",
        "QTSS_DETECTION_CLASSICAL_FLAG_PARALLEL", 0.3,
    ).await;
    let pennant_height = resolve_system_f64(
        pool, "detection", "classical.pennant_max_height_pct_of_pole",
        "QTSS_DETECTION_CLASSICAL_PENNANT_HEIGHT", 0.4,
    ).await;
    let chan_parallel = resolve_system_f64(
        pool, "detection", "classical.channel_parallelism_tol",
        "QTSS_DETECTION_CLASSICAL_CHAN_PARALLEL", 0.15,
    ).await;
    let chan_min_bars = resolve_system_u64(
        pool, "detection", "classical.channel_min_bars",
        "QTSS_DETECTION_CLASSICAL_CHAN_MIN_BARS", 20, 1, 500,
    ).await;
    let chan_min_slope = resolve_system_f64(
        pool, "detection", "classical.channel_min_slope_pct",
        "QTSS_DETECTION_CLASSICAL_CHAN_MIN_SLOPE", 0.001,
    ).await;
    let cup_min_bars = resolve_system_u64(
        pool, "detection", "classical.cup_min_bars",
        "QTSS_DETECTION_CLASSICAL_CUP_MIN_BARS", 30, 1, 1000,
    ).await;
    let cup_rim_eq = resolve_system_f64(
        pool, "detection", "classical.cup_rim_equality_tol",
        "QTSS_DETECTION_CLASSICAL_CUP_RIM_EQ", 0.03,
    ).await;
    let cup_min_depth = resolve_system_f64(
        pool, "detection", "classical.cup_min_depth_pct",
        "QTSS_DETECTION_CLASSICAL_CUP_MIN_DEPTH", 0.12,
    ).await;
    let cup_max_depth = resolve_system_f64(
        pool, "detection", "classical.cup_max_depth_pct",
        "QTSS_DETECTION_CLASSICAL_CUP_MAX_DEPTH", 0.50,
    ).await;
    let cup_r2 = resolve_system_f64(
        pool, "detection", "classical.cup_roundness_r2",
        "QTSS_DETECTION_CLASSICAL_CUP_R2", 0.6,
    ).await;
    let handle_max_depth = resolve_system_f64(
        pool, "detection", "classical.handle_max_depth_pct_of_cup",
        "QTSS_DETECTION_CLASSICAL_HANDLE_MAX", 0.5,
    ).await;
    let rounding_min_bars = resolve_system_u64(
        pool, "detection", "classical.rounding_min_bars",
        "QTSS_DETECTION_CLASSICAL_ROUND_MIN_BARS", 40, 1, 1000,
    ).await;
    let rounding_r2 = resolve_system_f64(
        pool, "detection", "classical.rounding_roundness_r2",
        "QTSS_DETECTION_CLASSICAL_ROUND_R2", 0.65,
    ).await;
    // Faz 10 Aşama 1 — Triple / Broadening / V / ABCD.
    let triple_peak_tol = resolve_system_f64(
        pool, "detection", "classical.triple_peak_tol",
        "QTSS_DETECTION_CLASSICAL_TRIPLE_PEAK_TOL", 0.03,
    ).await;
    let triple_min_span = resolve_system_u64(
        pool, "detection", "classical.triple_min_span_bars",
        "QTSS_DETECTION_CLASSICAL_TRIPLE_MIN_SPAN", 10, 1, 500,
    ).await;
    let triple_neck_slope = resolve_system_f64(
        pool, "detection", "classical.triple_neckline_slope_max",
        "QTSS_DETECTION_CLASSICAL_TRIPLE_NECK_SLOPE", 0.003,
    ).await;
    let broad_min_slope = resolve_system_f64(
        pool, "detection", "classical.broadening_min_slope_pct",
        "QTSS_DETECTION_CLASSICAL_BROAD_MIN_SLOPE", 0.002,
    ).await;
    let broad_flat_slope = resolve_system_f64(
        pool, "detection", "classical.broadening_flat_slope_pct",
        "QTSS_DETECTION_CLASSICAL_BROAD_FLAT_SLOPE", 0.0015,
    ).await;
    let v_max_bars = resolve_system_u64(
        pool, "detection", "classical.v_max_total_bars",
        "QTSS_DETECTION_CLASSICAL_V_MAX_BARS", 20, 1, 200,
    ).await;
    let v_min_amp = resolve_system_f64(
        pool, "detection", "classical.v_min_amplitude_pct",
        "QTSS_DETECTION_CLASSICAL_V_MIN_AMP", 0.03,
    ).await;
    let v_sym_tol = resolve_system_f64(
        pool, "detection", "classical.v_symmetry_tol",
        "QTSS_DETECTION_CLASSICAL_V_SYM_TOL", 0.4,
    ).await;
    let abcd_c_min = resolve_system_f64(
        pool, "detection", "classical.abcd_c_min_retrace",
        "QTSS_DETECTION_CLASSICAL_ABCD_C_MIN", 0.382,
    ).await;
    let abcd_c_max = resolve_system_f64(
        pool, "detection", "classical.abcd_c_max_retrace",
        "QTSS_DETECTION_CLASSICAL_ABCD_C_MAX", 0.886,
    ).await;
    let abcd_d_tol = resolve_system_f64(
        pool, "detection", "classical.abcd_d_projection_tol",
        "QTSS_DETECTION_CLASSICAL_ABCD_D_TOL", 0.15,
    ).await;
    let abcd_min_leg = resolve_system_u64(
        pool, "detection", "classical.abcd_min_bars_per_leg",
        "QTSS_DETECTION_CLASSICAL_ABCD_MIN_LEG", 3, 1, 200,
    ).await;
    // Faz 10 Aşama 4 — Scallop.
    let scallop_min_bars = resolve_system_u64(
        pool, "detection", "classical.scallop_min_bars",
        "QTSS_DETECTION_CLASSICAL_SCALLOP_MIN_BARS", 20, 1, 500,
    ).await;
    let scallop_progress = resolve_system_f64(
        pool, "detection", "classical.scallop_min_rim_progress_pct",
        "QTSS_DETECTION_CLASSICAL_SCALLOP_PROGRESS", 0.02,
    ).await;
    let scallop_r2 = resolve_system_f64(
        pool, "detection", "classical.scallop_roundness_r2",
        "QTSS_DETECTION_CLASSICAL_SCALLOP_R2", 0.55,
    ).await;

    ClassicalConfig {
        pivot_level: parse_pivot_level(&level_str),
        min_structural_score: min_score.clamp(0.0, 1.0),
        equality_tolerance: eq_tol.clamp(0.0, 0.25),
        apex_horizon_bars: apex_h,
        flatness_threshold_pct: flat_pct.clamp(0.0, 0.1),
        flatness_min_score: flat_min.clamp(0.0, 1.0),
        neckline_tolerance_mult: neck_mult.clamp(1.0, 3.0),
        triangle_symmetry_tol: tri_sym.clamp(0.0, 2.0),
        hs_max_neckline_slope_pct: hs_slope.clamp(0.0, 0.05),
        hs_time_symmetry_tol: hs_time.clamp(0.0, 2.0),
        rectangle_max_slope_pct: rect_slope.clamp(0.0, 0.1),
        rectangle_min_bars: rect_min_bars,
        flag_pole_min_move_atr: flag_pole_atr.clamp(0.0, 20.0),
        flag_pole_max_bars: flag_pole_bars,
        flag_max_retrace_pct: flag_retrace.clamp(0.0, 1.0),
        flag_atr_period: flag_atr_period,
        flag_parallelism_tol: flag_parallel.clamp(0.0, 2.0),
        pennant_max_height_pct_of_pole: pennant_height.clamp(0.0, 1.0),
        channel_parallelism_tol: chan_parallel.clamp(0.0, 2.0),
        channel_min_bars: chan_min_bars,
        channel_min_slope_pct: chan_min_slope.clamp(0.0, 0.1),
        cup_min_bars,
        cup_rim_equality_tol: cup_rim_eq.clamp(0.0, 0.25),
        cup_min_depth_pct: cup_min_depth.clamp(0.0, 1.0),
        cup_max_depth_pct: cup_max_depth.clamp(0.0, 1.0),
        cup_roundness_r2: cup_r2.clamp(0.0, 1.0),
        handle_max_depth_pct_of_cup: handle_max_depth.clamp(0.0, 1.0),
        rounding_min_bars,
        rounding_roundness_r2: rounding_r2.clamp(0.0, 1.0),
        triple_peak_tol: triple_peak_tol.clamp(0.0, 0.25),
        triple_min_span_bars: triple_min_span,
        triple_neckline_slope_max: triple_neck_slope.clamp(0.0, 0.05),
        broadening_min_slope_pct: broad_min_slope.clamp(0.0, 0.1),
        broadening_flat_slope_pct: broad_flat_slope.clamp(0.0, 0.1),
        v_max_total_bars: v_max_bars,
        v_min_amplitude_pct: v_min_amp.clamp(0.0, 0.5),
        v_symmetry_tol: v_sym_tol.clamp(0.0, 2.0),
        abcd_c_min_retrace: abcd_c_min.clamp(0.0, 1.0),
        abcd_c_max_retrace: abcd_c_max.clamp(0.0, 1.0),
        abcd_d_projection_tol: abcd_d_tol.clamp(0.0, 1.0),
        abcd_min_bars_per_leg: abcd_min_leg,
        scallop_min_bars,
        scallop_min_rim_progress_pct: scallop_progress.clamp(0.0, 0.5),
        scallop_roundness_r2: scallop_r2.clamp(0.0, 1.0),
    }
}

/// Resolve wyckoff detector config from system_config (CLAUDE.md #2).
async fn resolve_wyckoff_config(pool: &PgPool) -> WyckoffConfig {
    let level_str = resolve_system_string(
        pool, "detection", "wyckoff.pivot_level",
        "QTSS_DETECTION_WYCKOFF_PIVOT_LEVEL", "L0",
    ).await;
    let min_pivots = resolve_system_u64(
        pool, "detection", "wyckoff.min_range_pivots",
        "QTSS_DETECTION_WYCKOFF_MIN_RANGE_PIVOTS", 5, 4, 20,
    ).await as usize;
    let min_score = resolve_system_f64(
        pool, "detection", "wyckoff.min_structural_score",
        "QTSS_DETECTION_WYCKOFF_MIN_SCORE", 0.40,
    ).await as f32;
    let edge_tol = resolve_system_f64(
        pool, "detection", "wyckoff.range_edge_tolerance",
        "QTSS_DETECTION_WYCKOFF_EDGE_TOL", 0.04,
    ).await;
    let climax_vol = resolve_system_f64(
        pool, "detection", "wyckoff.climax_volume_mult",
        "QTSS_DETECTION_WYCKOFF_CLIMAX_VOL", 1.8,
    ).await;
    let min_pen = resolve_system_f64(
        pool, "detection", "wyckoff.min_penetration",
        "QTSS_DETECTION_WYCKOFF_MIN_PEN", 0.02,
    ).await;
    let max_pen = resolve_system_f64(
        pool, "detection", "wyckoff.max_penetration",
        "QTSS_DETECTION_WYCKOFF_MAX_PEN", 0.12,
    ).await;
    let shakeout_max_pen = resolve_system_f64(
        pool, "detection", "wyckoff.shakeout_max_penetration", "", 0.30,
    ).await;

    // Phase A
    let sc_vol_mult = resolve_system_f64(pool, "detector", "wyckoff.sc_volume_multiplier", "", 2.5).await;
    let sc_bar_mult = resolve_system_f64(pool, "detector", "wyckoff.sc_bar_width_multiplier", "", 2.0).await;
    let st_max_vol = resolve_system_f64(pool, "detector", "wyckoff.st_max_volume_ratio", "", 0.7).await;
    let ar_min_ret = resolve_system_f64(pool, "detector", "wyckoff.ar_min_retracement", "", 0.3).await;
    // Phase B
    let ua_exceed = resolve_system_f64(pool, "detector", "wyckoff.ua_max_exceed_pct", "", 0.03).await;
    let stb_decay = resolve_system_f64(pool, "detector", "wyckoff.stb_volume_decay_min", "", 0.85).await;
    // Phase C
    let shake_pen = resolve_system_f64(pool, "detector", "wyckoff.shakeout_min_penetration", "", 0.05).await;
    let shake_bars = resolve_system_u64(pool, "detector", "wyckoff.shakeout_recovery_bars", "", 3, 1, 20).await as usize;
    let manip_edge_tests = resolve_system_u64(pool, "detector", "wyckoff.manipulation_min_edge_tests", "", 3, 1, 10).await as usize;
    let manip_age_bars = resolve_system_u64(pool, "detector", "wyckoff.manipulation_min_range_age_bars", "", 20, 1, 500).await;
    let manip_edge_slope = resolve_system_f64(pool, "detector", "wyckoff.manipulation_max_edge_slope", "", 0.004).await;
    // P8 — pivot-window cap (see WyckoffConfig::pivot_window).
    let pivot_window = resolve_system_u64(pool, "detector", "wyckoff.pivot_window", "", 20, 8, 500).await as usize;
    // TF guards — caller sets these per-TF (H1 tighter than D1).
    let max_range_h_pct = resolve_system_f64(pool, "detector", "wyckoff.max_range_height_pct", "", 0.15).await;
    let max_range_age = resolve_system_u64(pool, "detector", "wyckoff.max_range_age_bars", "", 500, 20, 5000).await;
    let max_vol_expansion = resolve_system_f64(pool, "detector", "wyckoff.max_range_volume_expansion", "", 1.3).await;
    // Phase C test (P2d)
    let spring_test_vol = resolve_system_f64(pool, "detector", "wyckoff.spring_test_max_vol_ratio", "", 0.6).await;
    let spring_test_win = resolve_system_u64(pool, "detector", "wyckoff.spring_test_window_bars", "", 8, 1, 100).await;
    let spring_test_dist = resolve_system_f64(pool, "detector", "wyckoff.spring_test_max_distance", "", 0.10).await;
    // Spring variant thresholds (Pruden)
    let spr_ns_vol = resolve_system_f64(pool, "detector", "wyckoff.spring_no_supply_vol_ratio", "", 0.8).await;
    let spr_term_vol = resolve_system_f64(pool, "detector", "wyckoff.spring_terminal_vol_ratio", "", 3.0).await;
    let skip_term = resolve_worker_enabled_flag(pool, "detector", "wyckoff.skip_terminal_springs", "", true).await;
    // Phase B gate (P2c)
    let phase_b_min_bars = resolve_system_u64(pool, "detector", "wyckoff.phase_b_min_bars", "", 10, 0, 1000).await as usize;
    let phase_b_min_inner_tests = resolve_system_u64(pool, "detector", "wyckoff.phase_b_min_inner_tests", "", 1, 0, 10).await as usize;
    // Phase D
    let sos_vol = resolve_system_f64(pool, "detector", "wyckoff.sos_min_volume_ratio", "", 1.5).await;
    let sos_bar_atr = resolve_system_f64(pool, "detector", "wyckoff.sos_min_bar_width_atr_mult", "", 1.5).await;
    let sos_close_third = resolve_system_f64(pool, "detector", "wyckoff.sos_close_third_threshold", "", 0.66).await;
    let lps_ret = resolve_system_f64(pool, "detector", "wyckoff.lps_max_retracement", "", 0.5).await;
    let lps_vol = resolve_system_f64(pool, "detector", "wyckoff.lps_max_volume_ratio", "", 0.5).await;
    let creek_pct = resolve_system_f64(pool, "detector", "wyckoff.creek_level_percentile", "", 0.6).await;
    let jac_body = resolve_system_f64(pool, "detector", "wyckoff.jac_min_body_ratio", "", 0.5).await;
    let phase_b_climactic_mult = resolve_system_f64(pool, "detector", "wyckoff.phase_b_climactic_vol_flip_mult", "", 3.0).await;
    // Sloping / SOT
    let slope_thresh = resolve_system_f64(pool, "detector", "wyckoff.slope_threshold_deg", "", 5.0).await;
    let sot_decay = resolve_system_f64(pool, "detector", "wyckoff.sot_thrust_decay_ratio", "", 0.7).await;

    WyckoffConfig {
        pivot_level: parse_pivot_level(&level_str),
        min_range_pivots: min_pivots,
        range_edge_tolerance: edge_tol,
        climax_volume_mult: climax_vol,
        min_penetration: min_pen,
        max_penetration: max_pen,
        shakeout_max_penetration: shakeout_max_pen,
        min_structural_score: min_score,
        pivot_window,
        sc_volume_multiplier: sc_vol_mult,
        sc_bar_width_multiplier: sc_bar_mult,
        st_max_volume_ratio: st_max_vol,
        ar_min_retracement: ar_min_ret,
        ua_max_exceed_pct: ua_exceed,
        stb_volume_decay_min: stb_decay,
        phase_b_min_bars,
        phase_b_min_inner_tests,
        shakeout_min_penetration: shake_pen,
        shakeout_recovery_bars: shake_bars,
        manipulation_min_edge_tests: manip_edge_tests,
        manipulation_min_range_age_bars: manip_age_bars,
        manipulation_max_edge_slope: manip_edge_slope,
        max_range_height_pct: max_range_h_pct,
        max_range_age_bars: max_range_age,
        max_range_volume_expansion: max_vol_expansion,
        spring_test_max_vol_ratio: spring_test_vol,
        spring_test_window_bars: spring_test_win,
        spring_test_max_distance: spring_test_dist,
        spring_no_supply_vol_ratio: spr_ns_vol,
        spring_terminal_vol_ratio: spr_term_vol,
        skip_terminal_springs: skip_term,
        sos_min_volume_ratio: sos_vol,
        sos_min_bar_width_atr_mult: sos_bar_atr,
        sos_close_third_threshold: sos_close_third,
        lps_max_retracement: lps_ret,
        lps_max_volume_ratio: lps_vol,
        creek_level_percentile: creek_pct,
        jac_min_body_ratio: jac_body,
        phase_b_climactic_vol_flip_mult: phase_b_climactic_mult,
        slope_threshold_deg: slope_thresh,
        sot_thrust_decay_ratio: sot_decay,
    }
}

/// Resolve per-formation toggles for the Elliott detector set from
/// `system_config`. Each formation has its own `detection.elliott.<f>.enabled`
/// row — disabling one is a one-line UPDATE, no worker restart.
async fn resolve_elliott_toggles(pool: &PgPool) -> ElliottFormationToggles {
    // Lookup table: (field_setter, config_key, env_key, default).
    // Defined as a local closure list so adding a new formation is one
    // tuple, not a scattered if/else chain (CLAUDE.md #1).
    let mut t = ElliottFormationToggles::defaults();
    t.impulse = resolve_worker_enabled_flag(
        pool, "detection", "elliott.impulse.enabled",
        "QTSS_DETECTION_ELLIOTT_IMPULSE_ENABLED", true,
    ).await;
    t.leading_diagonal = resolve_worker_enabled_flag(
        pool, "detection", "elliott.leading_diagonal.enabled",
        "QTSS_DETECTION_ELLIOTT_LEADING_DIAGONAL_ENABLED", false,
    ).await;
    t.ending_diagonal = resolve_worker_enabled_flag(
        pool, "detection", "elliott.ending_diagonal.enabled",
        "QTSS_DETECTION_ELLIOTT_ENDING_DIAGONAL_ENABLED", false,
    ).await;
    t.zigzag = resolve_worker_enabled_flag(
        pool, "detection", "elliott.zigzag.enabled",
        "QTSS_DETECTION_ELLIOTT_ZIGZAG_ENABLED", false,
    ).await;
    t.flat = resolve_worker_enabled_flag(
        pool, "detection", "elliott.flat.enabled",
        "QTSS_DETECTION_ELLIOTT_FLAT_ENABLED", false,
    ).await;
    t.triangle = resolve_worker_enabled_flag(
        pool, "detection", "elliott.triangle.enabled",
        "QTSS_DETECTION_ELLIOTT_TRIANGLE_ENABLED", false,
    ).await;
    t.extended_impulse = resolve_worker_enabled_flag(
        pool, "detection", "elliott.extended_impulse.enabled",
        "QTSS_DETECTION_ELLIOTT_EXTENDED_IMPULSE_ENABLED", false,
    ).await;
    t.truncated_fifth = resolve_worker_enabled_flag(
        pool, "detection", "elliott.truncated_fifth.enabled",
        "QTSS_DETECTION_ELLIOTT_TRUNCATED_FIFTH_ENABLED", false,
    ).await;
    t.combination = resolve_worker_enabled_flag(
        pool, "detection", "elliott.combination.enabled",
        "QTSS_DETECTION_ELLIOTT_COMBINATION_ENABLED", false,
    ).await;
    t
}

/// Build the active detector list from config. Each family is gated by
/// its own `detection.<family>.enabled` toggle so an operator can
/// disable noisy ones without restarting the worker.
pub(crate) async fn build_runners(pool: &PgPool) -> Vec<Box<dyn DetectorRunner>> {
    let mut runners: Vec<Box<dyn DetectorRunner>> = Vec::new();

    if resolve_worker_enabled_flag(
        pool,
        "detection",
        "elliott.enabled",
        "QTSS_DETECTION_ELLIOTT_ENABLED",
        true,
    )
    .await
    {
        let toggles = resolve_elliott_toggles(pool).await;
        // Run Elliott at both L0 (fine) and L1 (coarse) pivot levels.
        // L0 catches more formations (like LuxAlgo's short zigzag),
        // L1 catches larger structural moves.
        for level in [PivotLevel::L0, PivotLevel::L1] {
            let mut cfg = ElliottConfig::defaults();
            cfg.pivot_level = level;
            match ElliottDetectorSet::new(cfg, &toggles) {
                Ok(set) => runners.push(Box::new(ElliottRunner(set))),
                Err(e) => warn!(?e, ?level, "elliott detector set init failed"),
            }
        }
    }
    if resolve_worker_enabled_flag(
        pool,
        "detection",
        "harmonic.enabled",
        "QTSS_DETECTION_HARMONIC_ENABLED",
        true,
    )
    .await
    {
        let harmonic_cfg = resolve_harmonic_config(pool).await;
        match HarmonicDetector::new(harmonic_cfg) {
            Ok(d) => runners.push(Box::new(HarmonicRunner(d))),
            Err(e) => warn!(?e, "harmonic detector init failed"),
        }
    }
    if resolve_worker_enabled_flag(
        pool,
        "detection",
        "classical.enabled",
        "QTSS_DETECTION_CLASSICAL_ENABLED",
        true,
    )
    .await
    {
        let classical_cfg = resolve_classical_config(pool).await;
        match ClassicalDetector::new(classical_cfg) {
            Ok(d) => runners.push(Box::new(ClassicalRunner(d))),
            Err(e) => warn!(?e, "classical detector init failed"),
        }
    }
    if resolve_worker_enabled_flag(
        pool,
        "detection",
        "wyckoff.enabled",
        "QTSS_DETECTION_WYCKOFF_ENABLED",
        true,
    )
    .await
    {
        let wyckoff_cfg = resolve_wyckoff_config(pool).await;
        match WyckoffDetector::new(wyckoff_cfg) {
            Ok(d) => runners.push(Box::new(WyckoffRunner(d))),
            Err(e) => warn!(?e, "wyckoff detector init failed"),
        }
    }
    if resolve_worker_enabled_flag(
        pool,
        "detection",
        "gap.enabled",
        "QTSS_DETECTION_GAP_ENABLED",
        true,
    )
    .await
    {
        let gap_cfg = resolve_gap_config(pool).await;
        match GapDetector::new(gap_cfg) {
            Ok(d) => runners.push(Box::new(GapRunner(d))),
            Err(e) => warn!(?e, "gap detector init failed"),
        }
    }
    if resolve_worker_enabled_flag(
        pool,
        "detection",
        "candle.enabled",
        "QTSS_DETECTION_CANDLE_ENABLED",
        true,
    )
    .await
    {
        let candle_cfg = resolve_candle_config(pool).await;
        match CandleDetector::new(candle_cfg) {
            Ok(d) => runners.push(Box::new(CandleRunner(d))),
            Err(e) => warn!(?e, "candle detector init failed"),
        }
    }
    if resolve_worker_enabled_flag(
        pool,
        "detection",
        "range.enabled",
        "QTSS_DETECTION_RANGE_ENABLED",
        true,
    )
    .await
    {
        runners.push(Box::new(RangeRunner::new(TradingRangeParams::default())));
    }
    if resolve_worker_enabled_flag(
        pool,
        "detection",
        "range_sub.enabled",
        "QTSS_DETECTION_RANGE_SUB_ENABLED",
        true,
    )
    .await
    {
        runners.push(Box::new(RangeSubRunner::new(RangeDetectorConfig::default())));
    }

    runners
}

// ---------------------------------------------------------------------
// Loop entry
// ---------------------------------------------------------------------

pub async fn v2_detection_orchestrator_loop(pool: PgPool) {
    info!("v2 detection orchestrator loop spawned (gated on detection.orchestrator.enabled)");
    let repo = Arc::new(V2DetectionRepository::new(pool.clone()));

    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            "detection",
            "orchestrator.enabled",
            "QTSS_DETECTION_ORCHESTRATOR_ENABLED",
            false,
        )
        .await;

        let tick_secs = resolve_system_u64(
            &pool,
            "detection",
            "orchestrator.tick_interval_s",
            "QTSS_DETECTION_ORCHESTRATOR_TICK_S",
            5,
            1,
            3600,
        )
        .await;

        if !enabled {
            tokio::time::sleep(Duration::from_secs(tick_secs)).await;
            continue;
        }

        match run_pass(&pool, repo.clone()).await {
            Ok(stats) => {
                if stats.inserted > 0 || stats.processed > 0 {
                    info!(
                        processed = stats.processed,
                        emitted = stats.emitted,
                        deduped = stats.deduped,
                        inserted = stats.inserted,
                        "v2 detection orchestrator pass complete"
                    );
                } else {
                    debug!("v2 detection orchestrator pass: no enabled symbols");
                }
            }
            Err(e) => warn!(%e, "v2 detection orchestrator pass failed"),
        }

        // Projection backfill: generate projections for wave_chain entries
        // that don't have any projections yet.
        if let Err(e) = backfill_projections(&pool).await {
            warn!(%e, "projection backfill failed");
        }

        // Projection validation: check active projections against latest prices.
        if let Err(e) = validate_active_projections(&pool).await {
            warn!(%e, "projection validation failed");
        }

        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
    }
}

#[derive(Default)]
struct PassStats {
    processed: usize,
    emitted: usize,
    deduped: usize,
    inserted: usize,
}

async fn run_pass(
    pool: &PgPool,
    repo: Arc<V2DetectionRepository>,
) -> anyhow::Result<PassStats> {
    let mut stats = PassStats::default();

    let history_bars = resolve_system_u64(
        pool,
        "detection",
        "orchestrator.history_bars",
        "QTSS_DETECTION_ORCHESTRATOR_HISTORY_BARS",
        500,
        50,
        5000,
    )
    .await as i64;

    let mode = resolve_system_string(
        pool,
        "worker",
        "runtime_mode",
        "QTSS_RUNTIME_MODE",
        "live",
    )
    .await;
    // Belt-and-braces: only accept the three documented modes; anything
    // else collapses to "live" so we never insert garbage in the CHECK.
    let mode = match mode.as_str() {
        "live" | "dry" | "backtest" => mode,
        _ => "live".to_string(),
    };

    let runners = Arc::new(build_runners(pool).await);
    if runners.is_empty() {
        return Ok(stats);
    }

    // P16 — parallelize per-symbol detection. Each symbol pulls its own
    // bars, runs the detector bank, inserts results. Bounded by
    // `detection.orchestrator.parallelism` (config-driven per CLAUDE.md
    // rule #2). Default 8 saturates typical 4-core worker without
    // overwhelming Postgres pool.
    let parallelism = resolve_system_u64(
        pool,
        "detection",
        "orchestrator.parallelism",
        "QTSS_DETECTION_ORCHESTRATOR_PARALLELISM",
        8,
        1,
        32,
    )
    .await as usize;

    let symbols = list_enabled_engine_symbols(pool).await?;

    let processed = AtomicUsize::new(0);
    let emitted = AtomicUsize::new(0);
    let deduped = AtomicUsize::new(0);
    let inserted = AtomicUsize::new(0);

    let mode_ref = &mode;
    let runners_ref = &runners;
    let repo_ref = &repo;
    let processed_ref = &processed;
    let emitted_ref = &emitted;
    let deduped_ref = &deduped;
    let inserted_ref = &inserted;

    stream::iter(symbols)
        .for_each_concurrent(parallelism, |sym| async move {
            if !qtss_storage::is_backfill_ready(pool, sym.id).await {
                debug!(symbol = %sym.symbol, interval = %sym.interval, "skip detection — backfill not complete");
                return;
            }
            match process_symbol(pool, repo_ref.clone(), &sym, history_bars, runners_ref, mode_ref).await {
                Ok(s) => {
                    processed_ref.fetch_add(1, Ordering::Relaxed);
                    emitted_ref.fetch_add(s.emitted, Ordering::Relaxed);
                    deduped_ref.fetch_add(s.deduped, Ordering::Relaxed);
                    inserted_ref.fetch_add(s.inserted, Ordering::Relaxed);
                }
                Err(e) => warn!(symbol = %sym.symbol, interval = %sym.interval, %e, "process_symbol failed"),
            }
        })
        .await;

    stats.processed = processed.load(Ordering::Relaxed);
    stats.emitted = emitted.load(Ordering::Relaxed);
    stats.deduped = deduped.load(Ordering::Relaxed);
    stats.inserted = inserted.load(Ordering::Relaxed);

    Ok(stats)
}

#[derive(Default)]
struct SymbolStats {
    emitted: usize,
    deduped: usize,
    inserted: usize,
}

async fn process_symbol(
    pool: &PgPool,
    repo: Arc<V2DetectionRepository>,
    sym: &EngineSymbolRow,
    history_bars: i64,
    runners: &Arc<Vec<Box<dyn DetectorRunner>>>,
    mode: &str,
) -> anyhow::Result<SymbolStats> {
    let mut stats = SymbolStats::default();

    let timeframe = match parse_timeframe(&sym.interval) {
        Some(tf) => tf,
        None => {
            debug!(interval = %sym.interval, "skip: unsupported timeframe");
            return Ok(stats);
        }
    };
    let instrument = build_instrument(&sym.exchange, &sym.segment, &sym.symbol);

    let raw_bars = list_recent_bars(
        pool,
        &sym.exchange,
        &sym.segment,
        &sym.symbol,
        &sym.interval,
        history_bars,
    )
    .await?;
    if raw_bars.len() < 30 {
        // Not enough history to even warm up indicators.
        return Ok(stats);
    }

    // list_recent_bars returns DESC; feed engines in chronological order.
    let mut chronological = raw_bars;
    chronological.reverse();

    // Adaptive ATR multipliers: fewer bars → lower thresholds to produce
    // enough pivots for pattern detection (need ≥6 for impulse).
    let pivot_cfg = if chronological.len() <= 120 {
        // Low-bar TFs (e.g. 1M with ~80 bars): halve the multipliers
        PivotConfig {
            atr_period: 10,
            atr_mult: [
                Decimal::new(8, 1),   // 0.8
                Decimal::new(16, 1),  // 1.6
                Decimal::new(32, 1),  // 3.2
                Decimal::new(64, 1),  // 6.4
            ],
        }
    } else {
        PivotConfig::defaults()
    };
    let mut pivot_engine = PivotEngine::new(pivot_cfg)?;
    let mut regime_engine = RegimeEngine::new(RegimeConfig::defaults())?;

    let mut latest_regime: Option<RegimeSnapshot> = None;
    let mut bars: Vec<Bar> = Vec::with_capacity(chronological.len());
    for row in &chronological {
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
        // Pivot + regime cascade. Errors here mean out-of-order bars
        // — log and stop processing this symbol so we don't poison
        // the engine state with retries on bad data.
        if let Err(e) = pivot_engine.on_bar(&bar) {
            warn!(symbol = %sym.symbol, %e, "pivot engine rejected bar");
            return Ok(stats);
        }
        match regime_engine.on_bar(&bar) {
            Ok(Some(snap)) => latest_regime = Some(snap),
            Ok(None) => {}
            Err(e) => {
                warn!(symbol = %sym.symbol, %e, "regime engine rejected bar");
                return Ok(stats);
            }
        }
        bars.push(bar);
    }

    let Some(regime) = latest_regime else {
        // Indicators still warming up — try again next tick.
        return Ok(stats);
    };
    let tree = pivot_engine.snapshot();

    // Debug: log pivot counts per level for diagnosis
    info!(
        symbol = %sym.symbol,
        interval = %sym.interval,
        bars = bars.len(),
        L0 = tree.count(PivotLevel::L0),
        L1 = tree.count(PivotLevel::L1),
        L2 = tree.count(PivotLevel::L2),
        L3 = tree.count(PivotLevel::L3),
        "pivot tree built"
    );

    // ── Pivot cache: write newly computed pivots ──────────────────────
    // For each level, find pivots with bar_index > max cached and batch-
    // upsert them. This makes future ticks cheaper: only new bars need
    // pivot extraction; everything else comes from DB.
    {
        let levels = [
            (PivotLevel::L0, "L0"),
            (PivotLevel::L1, "L1"),
            (PivotLevel::L2, "L2"),
            (PivotLevel::L3, "L3"),
        ];
        for (level, level_str) in &levels {
            let cached_max = max_cached_bar_index(
                pool,
                &sym.exchange,
                &sym.symbol,
                &sym.interval,
                level_str,
            )
            .await
            .unwrap_or(None)
            .unwrap_or(-1);

            let new_pivots: Vec<PivotCacheRow> = tree
                .at_level(*level)
                .iter()
                .filter(|p| p.bar_index as i64 > cached_max)
                .map(|p| PivotCacheRow {
                    exchange: sym.exchange.clone(),
                    symbol: sym.symbol.clone(),
                    timeframe: sym.interval.clone(),
                    level: level_str.to_string(),
                    bar_index: p.bar_index as i64,
                    open_time: p.time,
                    price: p.price,
                    kind: match p.kind {
                        PivotKind::High => "High".to_string(),
                        PivotKind::Low => "Low".to_string(),
                    },
                    prominence: p.prominence,
                    volume_at_pivot: p.volume_at_pivot,
                    swing_type: p.swing_type.map(|s| format!("{:?}", s)),
                })
                .collect();

            if !new_pivots.is_empty() {
                match upsert_pivot_cache_batch(pool, &new_pivots).await {
                    Ok(n) => debug!(
                        symbol = %sym.symbol,
                        interval = %sym.interval,
                        level = %level_str,
                        count = n,
                        "pivot_cache: wrote new pivots"
                    ),
                    Err(e) => warn!(
                        symbol = %sym.symbol,
                        %e,
                        "pivot_cache upsert failed"
                    ),
                }
            }
        }
    }

    // Diagnostic: per-symbol pivot count at each level. Logged once per
    // symbol so we can verify detectors that require L1 (Harmonic, Wyckoff)
    // actually have enough data.
    debug!(
        symbol = %sym.symbol,
        interval = %sym.interval,
        bars = bars.len(),
        l0 = tree.at_level(PivotLevel::L0).len(),
        l1 = tree.at_level(PivotLevel::L1).len(),
        l2 = tree.at_level(PivotLevel::L2).len(),
        l3 = tree.at_level(PivotLevel::L3).len(),
        "pivot tree snapshot"
    );

    // Live-revision pre-pass: invalidate any open `forming` row whose
    // invalidation_price has been breached by the most recent close.
    // This catches the case where the detector simply *stops* emitting
    // a wave (because it broke) — without this sweep the old forming
    // overlay would linger until something else supersedes it. See
    // post-Faz 8.0 backlog item #2.
    if let Some(last_bar) = bars.last() {
        let last_close = last_bar.close;
        match repo
            .list_forming_for_symbol(&sym.exchange, &sym.symbol, &sym.interval)
            .await
        {
            Ok(open_rows) => {
                let last_bar_time = last_bar.open_time;
                for row in open_rows {
                    // 1) Invalidation: price breached invalidation level
                    if let Some(dir) = infer_direction(&row.family, &row.subkind) {
                        let breached = match dir {
                            Direction::Long => last_close < row.invalidation_price,
                            Direction::Short => last_close > row.invalidation_price,
                        };
                        if breached {
                            if let Err(e) = repo.update_state(row.id, "invalidated").await {
                                warn!(id=%row.id, %e, "price-breach invalidate failed");
                            }
                            continue;
                        }
                    }
                    // 2) Confirmation: last anchor is well in the past
                    //    (at least 3 bars ago) and invalidation not breached
                    //    → the formation completed successfully.
                    if let Some(last_anchor_t) = row.last_anchor_time() {
                        if last_anchor_t < last_bar_time {
                            if let Err(e) = repo.update_state(row.id, "confirmed").await {
                                warn!(id=%row.id, %e, "confirm detection failed");
                            }
                        }
                    }
                }
            }
            Err(e) => warn!(symbol = %sym.symbol, %e, "list_forming_for_symbol failed"),
        }
    }

    for runner in runners.iter() {
        let detections = runner.detect(&tree, &bars, &instrument, timeframe, &regime);
        if detections.is_empty() {
            debug!(
                symbol = %sym.symbol,
                interval = %sym.interval,
                family = runner.family(),
                "detector returned 0 detections"
            );
        }
        for detection in detections {
        stats.emitted += 1;

        let (family, subkind) = split_pattern_kind(&detection.kind);
        let last_anchor_idx = detection.anchors.last().map(|a| a.bar_index).unwrap_or(0);

        if let Some(existing_id) = dedup_open(
            repo.as_ref(),
            &sym.exchange,
            &sym.symbol,
            &sym.interval,
            family,
            subkind,
            last_anchor_idx,
        )
        .await?
        {
            // Same structure detected — don't insert a duplicate, but
            // refresh the projection data so forecasts stay up-to-date
            // as new bars close (fixes frozen forecast bug).
            let bar_interval = chronological
                .windows(2)
                .last()
                .map(|w| w[1].open_time - w[0].open_time)
                .unwrap_or_else(|| chrono::Duration::seconds(60));
            let last_chrono_idx = chronological.len().saturating_sub(1) as u64;
            let last_chrono_time = chronological
                .last()
                .map(|r| r.open_time)
                .unwrap_or_else(chrono::Utc::now);
            let projected_json = json!(detection
                .projected_anchors
                .iter()
                .map(|a| {
                    let offset = a.bar_index.saturating_sub(last_chrono_idx) as i32;
                    let proj_time = last_chrono_time + bar_interval * offset;
                    json!({
                        "bar_index": a.bar_index,
                        "time": proj_time.to_rfc3339(),
                        "price": a.price.to_string(),
                        "level": format!("{:?}", a.level),
                        "label": a.label,
                    })
                })
                .collect::<Vec<_>>());
            // ── Projection accuracy check ──────────────────────────
            // Compare projected anchor prices with actual bar closes.
            // If a projected anchor's bar_index has been reached, measure
            // the deviation. Large deviations decay the structural_score;
            // accurate projections boost it (capped at 1.0).
            let mut accuracy_score = detection.structural_score;
            let mut projection_hits = 0u32;
            let mut projection_misses = 0u32;
            let current_bar_count = chronological.len() as u64;
            let _last_close = chronological
                .last()
                .map(|r| r.close)
                .unwrap_or_default();

            for pa in &detection.projected_anchors {
                if pa.bar_index >= current_bar_count {
                    continue; // not yet reached
                }
                // Find the actual bar at projected index
                if let Some(actual_bar) = chronological.get(pa.bar_index as usize) {
                    let actual_close = actual_bar.close;
                    let projected_price = pa.price;
                    let deviation = if projected_price != Decimal::ZERO {
                        ((actual_close - projected_price).abs() / projected_price)
                            .to_f32()
                            .unwrap_or(1.0)
                    } else {
                        1.0
                    };
                    // <5% deviation = hit, >15% = miss
                    if deviation < 0.05 {
                        projection_hits += 1;
                        accuracy_score = (accuracy_score + 0.02).min(1.0);
                    } else if deviation > 0.15 {
                        projection_misses += 1;
                        accuracy_score = (accuracy_score - 0.05).max(0.0);
                    }
                }
            }

            // If too many misses, invalidate the projection
            if projection_misses >= 2 && projection_hits == 0 {
                if let Err(e) = repo.update_state(existing_id, "invalidated").await {
                    warn!(%existing_id, %e, "projection-accuracy invalidate failed");
                }
                stats.deduped += 1;
                continue;
            }

            let updated_meta = json!({
                "detection_id": detection.id,
                "last_anchor_idx": last_anchor_idx,
                "structural_score": accuracy_score,
                "projected_anchors": projected_json,
                "projection_hits": projection_hits,
                "projection_misses": projection_misses,
            });
            if let Err(e) = repo.update_projection(
                existing_id,
                accuracy_score,
                updated_meta,
            ).await {
                warn!(symbol = %sym.symbol, %e, "update_projection on dedup failed");
            }
            stats.deduped += 1;
            continue;
        }

        // Enrich each pivot with the bar's open_time so the chart can
        // draw a polyline directly without a second round-trip. The
        // detector only carries bar_index → we resolve it here against
        // the chronological window we just fed the engines.
        let anchors_json = json!(detection
            .anchors
            .iter()
            .map(|a| {
                let idx = a.bar_index as usize;
                let time = chronological
                    .get(idx)
                    .map(|r| r.open_time.to_rfc3339())
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
        let regime_json =
            serde_json::to_value(&detection.regime_at_detection).unwrap_or_else(|_| json!({}));
        // Forward-projected anchors (Faz 7.6 / A2). Same JSON shape as
        // the realized anchors above so the chart can render them with
        // the same polyline machinery — only the line style differs
        // (dashed for projections, see Faz 7.6 / A4). Projection target
        // bars are *future* bars that don't exist in `chronological`,
        // so we synthesize their `time` by extrapolating one bar
        // interval forward from the last realized anchor.
        let bar_interval = chronological
            .windows(2)
            .last()
            .map(|w| w[1].open_time - w[0].open_time)
            .unwrap_or_else(|| chrono::Duration::seconds(60));
        let last_chrono_idx = chronological.len().saturating_sub(1) as u64;
        let last_chrono_time = chronological
            .last()
            .map(|r| r.open_time)
            .unwrap_or_else(chrono::Utc::now);
        let projected_json = json!(detection
            .projected_anchors
            .iter()
            .map(|a| {
                let offset = a.bar_index.saturating_sub(last_chrono_idx) as i32;
                let proj_time = last_chrono_time + bar_interval * offset;
                json!({
                    "bar_index": a.bar_index,
                    "time": proj_time.to_rfc3339(),
                    "price": a.price.to_string(),
                    "level": format!("{:?}", a.level),
                    "label": a.label,
                })
            })
            .collect::<Vec<_>>());
        // Sub-wave decomposition (Faz 7.6 / A3). Sub-wave bar indices
        // fall *inside* the realized window, so we can resolve their
        // `time` straight from `chronological` like we do for the main
        // anchors.
        let sub_waves_json = json!(detection
            .sub_wave_anchors
            .iter()
            .map(|seg| {
                seg.iter()
                    .map(|a| {
                        let idx = a.bar_index as usize;
                        let time = chronological
                            .get(idx)
                            .map(|r| r.open_time.to_rfc3339())
                            .unwrap_or_default();
                        json!({
                            "bar_index": a.bar_index,
                            "time": time,
                            "price": a.price.to_string(),
                            "level": format!("{:?}", a.level),
                            "label": a.label,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>());
        let raw_meta = json!({
            "detection_id": detection.id,
            "last_anchor_idx": last_anchor_idx,
            "structural_score": detection.structural_score,
            "projected_anchors": projected_json,
            "sub_wave_anchors": sub_waves_json,
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
        let inserted_id = new_row.id;
        let fs_raw_meta = new_row.raw_meta.clone();
        let fs_family = new_row.family;
        let fs_subkind = new_row.subkind.to_string();
        let fs_anchors = new_row.anchors.clone();
        let fs_structural = new_row.structural_score;
        let fs_invalidation = new_row.invalidation_price;
        repo.insert(new_row).await?;
        stats.inserted += 1;

        // Faz 9.0.2 / Faz 9.8.AI-Yol2 — feature store hook: per-detection
        // ConfluenceSource extraction → qtss_features_snapshot. Best-effort,
        // logged-only on failure so detector pipeline never blocks.
        //
        // The JSON below is the contract every `ConfluenceSource::extract`
        // reads. Keep family-specific fields at the top level (phase/events
        // for wyckoff, subkind/anchors/raw_meta for structural detectors)
        // so new sources can pick what they need without ripping the hook
        // apart (CLAUDE.md #1: dispatch on `family` inside the source, not
        // with an if-chain here).
        {
            use serde_json::json;
            let raw_for_fs = json!({
                "family": fs_family,
                "subkind": fs_subkind,
                "structural_score": fs_structural,
                "invalidation_price": fs_invalidation.to_string(),
                "anchors": fs_anchors,
                "raw_meta": fs_raw_meta,
                // Flattened conveniences for the existing wyckoff source
                // (backwards-compatible; the source still reads these).
                "phase": fs_raw_meta.get("phase"),
                "events_json": fs_raw_meta.get("events_json"),
                "range_bars": fs_raw_meta.get("range_bars"),
                "is_accumulation": fs_raw_meta.get("is_accumulation"),
                "is_distribution": fs_raw_meta.get("is_distribution"),
            });
            let store = crate::feature_store::FeatureStore::new(pool);
            if let Err(e) = store
                .write_for_detection(
                    inserted_id,
                    None,
                    &sym.exchange,
                    &sym.symbol,
                    &sym.interval,
                    None,
                    &raw_for_fs,
                )
                .await
            {
                tracing::warn!(%e, detection_id = %inserted_id, "feature_store write");
            }
        }

        // Live revision: retire any older `forming` row for the same
        // (symbol, tf, family, subkind). Each new bar tick produces a
        // fresh detection that should *replace* the previous overlay,
        // not stack on top of it. See post-Faz 8.0 backlog item #2.
        if let Err(e) = repo
            .supersede_previous_forming(
                &sym.exchange,
                &sym.symbol,
                &sym.interval,
                family,
                subkind,
                inserted_id,
            )
            .await
        {
            warn!(symbol = %sym.symbol, family, subkind, %e, "supersede_previous_forming failed");
        }
        // Wyckoff structure tracker: when a wyckoff family detection is
        // inserted, feed it into the persistent structure state machine.
        if family == "wyckoff" {
            if let Err(e) = upsert_wyckoff_structure_from_detection(
                pool, &sym.exchange, &sym.symbol, &sym.interval, subkind,
                &detection, inserted_id, &chronological,
            ).await {
                warn!(symbol = %sym.symbol, %e, "wyckoff structure upsert failed");
            }
        }

        // Elliott Deep: link detection to wave_chain hierarchy.
        if family == "elliott" {
            if let Err(e) = link_elliott_to_wave_chain(
                pool,
                &sym.exchange,
                &sym.symbol,
                &sym.interval,
                timeframe,
                &detection,
                inserted_id,
                &chronological,
            )
            .await
            {
                warn!(symbol = %sym.symbol, %e, "wave_chain link failed");
            }
        }
        }
    }

    Ok(stats)
}

/// Feed a Wyckoff detection into the persistent structure tracker.
/// Creates a new structure if none exists, or updates the existing one
/// with the new event.
pub(crate) async fn upsert_wyckoff_structure_from_detection(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    interval: &str,
    subkind: &str,
    detection: &Detection,
    _detection_id: Uuid,
    chronological: &[MarketBarRow],
) -> anyhow::Result<()> {
    use qtss_storage::{
        complete_wyckoff_structure, fail_wyckoff_structure, find_active_wyckoff_structure,
        insert_wyckoff_structure, update_wyckoff_structure, WyckoffStructureInsert,
    };
    use qtss_wyckoff::{
        WyckoffEvent, WyckoffPhase, WyckoffSchematic, WyckoffStructureTracker,
    };

    // Parse event from subkind (e.g. "selling_climax_accumulation" → "selling_climax")
    let event_name = subkind.rsplit('_').skip(1).collect::<Vec<_>>().into_iter().rev()
        .collect::<Vec<_>>().join("_");
    let variant = subkind.rsplit('_').next().unwrap_or("");

    let wy_event = match WyckoffStructureTracker::event_from_detector_name(&event_name) {
        Some(e) => e,
        None => {
            // trading_range and some events don't map directly
            // Try the full subkind minus the variant
            let alt = subkind.strip_suffix(&format!("_{variant}")).unwrap_or(subkind);
            match WyckoffStructureTracker::event_from_detector_name(alt) {
                Some(e) => e,
                None => return Ok(()), // Not a trackable event
            }
        }
    };

    let last_anchor = detection.anchors.last();
    let price = last_anchor
        .map(|a| a.price.to_f64().unwrap_or(0.0))
        .unwrap_or(0.0);
    let bar_idx = last_anchor.map(|a| a.bar_index).unwrap_or(0);
    // Resolve the anchor bar's open time from the chronological window
    // the orchestrator fed to the detectors. Enables the chart overlay
    // to pin the event to the exact candle regardless of bar_index
    // origin (rolling-window vs global post-P2a).
    let time_ms = chronological
        .get(bar_idx as usize)
        .map(|r| r.open_time.timestamp_millis());

    // Check if there's an active structure for this symbol/interval
    let existing = find_active_wyckoff_structure(pool, symbol, interval).await?;

    // Structure TTL + chronology guard.
    //
    // A Wyckoff structure is a localised phenomenon (range + manipulation
    // + resolution). If the last recorded event is more than `ttl_s`
    // seconds behind the incoming event — measured purely from each
    // event's own `time_ms` — the active row is not the same structure.
    //
    // P28c (2026-04) — the prior implementation (P28a) fell back to
    // `chrono::Utc::now()` whenever the incoming event had no `time_ms`,
    // which happens constantly on historical scans where the anchor
    // `bar_idx` is out of range of the rolling `chronological` window.
    // Wall-clock minus a 2021 event ≈ 140M seconds, far beyond any TTL
    // → every structure died on its 2nd event, none reached Phase C.
    // Verdict from Faz 8.2 diagnostic (383 structs, 0 completed).
    //
    // Now we ONLY expire when both sides have a concrete `time_ms` and
    // the incoming event is strictly newer than the last recorded event
    // by more than `ttl_s`. Missing `time_ms` → withhold the guard
    // (structure survives); family-flip detection below still catches
    // cross-cycle corruption. Non-monotonic arrivals (backfill
    // interleaving) are dropped outright rather than poisoning phase
    // derivation — see the chronology guard immediately after.
    if let Some(ref row) = existing {
        let ttl_s = resolve_wyckoff_ttl_seconds(pool, interval).await;
        let events: Vec<qtss_wyckoff::RecordedEvent> =
            serde_json::from_value(row.events_json.clone()).unwrap_or_default();
        let last_time_ms = events.iter().rev().find_map(|e| e.time_ms);

        // Chronology guard: drop events that would rewrite history.
        // Returning Ok early is preferable to silently corrupting
        // `events_json` with out-of-order entries — the next in-order
        // event will re-trigger this code path cleanly.
        if let (Some(lm), Some(now)) = (last_time_ms, time_ms) {
            if now < lm {
                tracing::warn!(
                    %symbol, %interval,
                    delta_s = (lm - now) / 1000,
                    event = %wy_event.as_str(),
                    "wyckoff: dropped non-monotonic event (chronology guard)",
                );
                return Ok(());
            }
        }

        let expire = matches!(
            (last_time_ms, time_ms),
            (Some(lm), Some(now)) if (now - lm) / 1000 > ttl_s as i64
        );
        if expire {
            let (lm, now) = (last_time_ms.unwrap(), time_ms.unwrap());
            let reason = format!(
                "structure TTL exceeded ({}s since last event; limit {}s on {})",
                (now - lm) / 1000,
                ttl_s,
                interval,
            );
            qtss_storage::fail_wyckoff_structure(pool, row.id, &reason).await?;
            return Box::pin(upsert_wyckoff_structure_from_detection(
                pool, exchange, symbol, interval, subkind, detection, _detection_id, chronological,
            )).await;
        }
    }

    // Faz 8.2 — failed Phase-C watchdog window (bars after Spring/UTAD
    // within which a contrary pivot invalidates the trigger). Previously
    // hardcoded to 12 in structure.rs (CLAUDE.md #2 violation).
    let failed_phase_c_window = qtss_storage::resolve_system_u64(
        pool,
        "wyckoff",
        "failed_phase_c_window_bars",
        "QTSS_WYCKOFF_FAILED_PHASE_C_WINDOW_BARS",
        12,
        1,
        500,
    )
    .await;

    match existing {
        Some(row) => {
            // Deserialize tracker state from events_json
            let events: Vec<qtss_wyckoff::RecordedEvent> =
                serde_json::from_value(row.events_json.clone()).unwrap_or_default();
            let schematic = match row.schematic.as_str() {
                "accumulation" => WyckoffSchematic::Accumulation,
                "distribution" => WyckoffSchematic::Distribution,
                "reaccumulation" => WyckoffSchematic::ReAccumulation,
                "redistribution" => WyckoffSchematic::ReDistribution,
                _ => WyckoffSchematic::Accumulation,
            };
            let mut tracker = WyckoffStructureTracker::new(
                schematic,
                row.range_top.unwrap_or(0.0),
                row.range_bottom.unwrap_or(0.0),
            )
            .with_phase_b_gate(
                resolve_system_u64(pool, "detector", "wyckoff.phase_b_min_inner_tests", "", 1, 0, 10).await as usize,
                resolve_system_u64(pool, "detector", "wyckoff.phase_b_min_bars", "", 10, 0, 1000).await as usize,
            )
            .with_phase_gates(
                resolve_worker_enabled_flag(pool, "detector", "wyckoff.phase.a_to_b.require_st", "", false).await,
                resolve_system_u64(pool, "detector", "wyckoff.phase.min_dwell_bars", "", 3, 0, 100).await as usize,
            )
            .with_failed_phase_c_window(failed_phase_c_window);
            tracker.creek = row.creek_level;
            tracker.ice = row.ice_level;
            tracker.events = events;
            // Re-derive phase from events
            for ev in &tracker.events {
                let _ = ev; // phase is re-derived in record_event
            }

            // Record new event
            let prior_schematic = row.schematic.clone();
            tracker.record_event_with_time(
                wy_event, bar_idx, price, detection.structural_score as f64, time_ms,
            );

            // Lifecycle: detect Phase E completion or directional flip (failure).
            //
            // #2 Phase E closure — terminal phase means markup/markdown confirmed;
            //    mark structure complete so the next PS/SC can spawn a fresh tracker
            //    (find_active filters by is_active).
            //
            // #3 Failed Spring/UTAD regression — auto_reclassify already flips the
            //    schematic when an event of the opposite directional family fires
            //    (e.g. Accumulation seeing UTAD/SOW/LPSY/BreakOfIce → ReDistribution).
            //    A flip of the bull/bear family relative to the persisted schematic
            //    is the canonical Wyckoff "failed structure" signal; flag it failed
            //    rather than letting the tracker silently mutate.
            let was_bull = matches!(prior_schematic.as_str(), "accumulation" | "reaccumulation");
            let now_bull = matches!(
                tracker.schematic,
                WyckoffSchematic::Accumulation | WyckoffSchematic::ReAccumulation
            );
            // Faz 10 pre-step — schematic-lock delay. Production data
            // showed Phase A/B flips dominating the FAILED bucket
            // (screenshot: distribution → reaccumulation via LPS while
            // still on Phase A). Wyckoff literature defers the real
            // directional commitment to Phase C (Spring / UTAD). Before
            // the lock phase, let auto_reclassify mutate the schematic
            // silently — only flag as FAILED once the structure was
            // already past the configured lock phase when the flip
            // happened. Lock phase is config-driven (CLAUDE.md #2).
            let lock_phase_str = qtss_storage::resolve_system_string(
                pool, "detector", "wyckoff.schematic_lock_phase",
                "", "C",
            )
            .await;
            let lock_phase = match lock_phase_str.to_ascii_uppercase().as_str() {
                "A" => WyckoffPhase::A,
                "B" => WyckoffPhase::B,
                "C" => WyckoffPhase::C,
                "D" => WyckoffPhase::D,
                "E" => WyckoffPhase::E,
                _ => WyckoffPhase::C,
            };
            let phase_locked = tracker.current_phase >= lock_phase;
            let family_flipped = was_bull != now_bull && phase_locked;

            let events_json = serde_json::to_value(&tracker.events)?;
            update_wyckoff_structure(
                pool,
                row.id,
                tracker.current_phase.as_str(),
                tracker.schematic.as_str(),
                tracker.range_top,
                tracker.range_bottom,
                tracker.creek,
                tracker.ice,
                &events_json,
                tracker.confidence(),
            )
            .await?;

            // Phase E trigger: no detector emits Markup/Markdown
            // directly, and JAC / BreakOfIce are rare on real data, so
            // Phase D structures would never complete. Here we inject a
            // synthetic Markup/Markdown event when the last ~30 bars
            // show a sustained breakout beyond the established range,
            // consistent with the tracker's directional bias.
            if tracker.current_phase == WyckoffPhase::D {
                let inj = resolve_markup_inject_config(pool).await;
                maybe_inject_markup_markdown(&mut tracker, chronological, time_ms, bar_idx, &inj);
                // record_event_with_time may have advanced to E.
                let events_json = serde_json::to_value(&tracker.events)?;
                let _ = update_wyckoff_structure(
                    pool, row.id,
                    tracker.current_phase.as_str(),
                    tracker.schematic.as_str(),
                    tracker.range_top, tracker.range_bottom,
                    tracker.creek, tracker.ice,
                    &events_json, tracker.confidence(),
                ).await;
            }

            if tracker.current_phase == WyckoffPhase::E {
                complete_wyckoff_structure(pool, row.id).await?;
            } else if family_flipped {
                let reason = format!(
                    "schematic flipped {} → {} via {}",
                    prior_schematic,
                    tracker.schematic.as_str(),
                    wy_event.as_str()
                );
                fail_wyckoff_structure(pool, row.id, &reason).await?;
            }
        }
        None => {
            // A new structure must seed with a Phase A event (PS/SC/BC/AR/ST).
            // A bare Phase C/D/E event with no active parent means we
            // missed the earlier structure — spawning a fresh row from
            // it produces a misleading "Phase D without A/B/C" record
            // (operator caught this in production). Skip instead; the
            // event will be re-picked up once a Phase A event seeds
            // a proper structure, or it belongs to history we've
            // already closed.
            if wy_event.phase() != WyckoffPhase::A {
                return Ok(());
            }
            // P28b — seed schematic from the *directional family* of
            // the Phase-A climax, not from a Spring/UTAD/SOS lookup
            // (those are Phase C/D events and never reach this branch
            // anyway thanks to the guard above). SC/PS → Accumulation,
            // BC → Distribution, AR/ST inherit the detection.variant
            // since they are direction-neutral in isolation.
            //
            // Prior heuristic mapped SOS→Accum / everything-else→Distrib
            // which silently put AR into the distribution bucket and
            // let a subsequent bullish event trigger auto_reclassify →
            // "schematic flipped" failure. Now direction is sourced
            // authoritatively from the detection payload, with a
            // climax-kind fallback only when variant is missing.
            let schematic = match variant {
                "accumulation" | "reaccumulation" => WyckoffSchematic::Accumulation,
                "distribution" | "redistribution" => WyckoffSchematic::Distribution,
                _ => match wy_event {
                    WyckoffEvent::SC | WyckoffEvent::PS => WyckoffSchematic::Accumulation,
                    WyckoffEvent::BC => WyckoffSchematic::Distribution,
                    // AR / ST alone cannot disambiguate direction.
                    // Drop the event instead of guessing — it will be
                    // picked up by the next climax-anchored seed.
                    _ => return Ok(()),
                },
            };

            let mut tracker = WyckoffStructureTracker::new(schematic, price, price)
                .with_failed_phase_c_window(failed_phase_c_window);
            tracker.record_event_with_time(
                wy_event, bar_idx, price, detection.structural_score as f64, time_ms,
            );

            // Use detection anchors to estimate range
            let mut hi = f64::MIN;
            let mut lo = f64::MAX;
            for a in &detection.anchors {
                let p = a.price.to_f64().unwrap_or(0.0);
                if p > hi { hi = p; }
                if p < lo { lo = p; }
            }
            if hi > lo {
                tracker.range_top = hi;
                tracker.range_bottom = lo;
            }

            // P7 — TF guard at structure birth.
            //
            // SQL dump showed BTCUSDT 1d structures with range 3621 → 126208
            // (height/midpoint ≈ 189%). `eval_trading_range` applies the
            // height cap, but individual event detectors (SC/BC/…) bypass
            // it and feed `detection.anchors` directly here. Apply the cap
            // per-TF on the birth path so a pivot set spanning an entire
            // exchange's history cannot seed a Wyckoff structure.
            //
            // Per-TF override: `wyckoff.max_range_height_pct.<interval>`;
            // falls back to the global `wyckoff.max_range_height_pct`.
            let default_cap_by_tf: f64 = match interval {
                "1m" | "3m" | "5m"   => 0.05,
                "15m" | "30m"        => 0.07,
                "1h" | "2h"          => 0.10,
                "4h" | "6h" | "8h"   => 0.15,
                "12h" | "1d"         => 0.25,
                "3d" | "1w"          => 0.35,
                "1M"                 => 0.50,
                _                    => 0.15,
            };
            let cap_key = format!("wyckoff.max_range_height_pct.{interval}");
            let cap = resolve_system_f64(pool, "detector", &cap_key, "", default_cap_by_tf).await;
            let mid = (tracker.range_top + tracker.range_bottom) * 0.5;
            if mid > 0.0 {
                // Reject inverted ranges outright (resistance <= support).
                // Can happen if a late AR/BC mutation in structure.rs writes a
                // price below the prior support — the structure is corrupt
                // and cannot be repaired downstream.
                let raw = tracker.range_top - tracker.range_bottom;
                if raw <= 0.0 {
                    tracing::info!(
                        symbol = %symbol,
                        interval = %interval,
                        event = %wy_event.as_str(),
                        range_bottom = %tracker.range_bottom,
                        range_top = %tracker.range_top,
                        "wyckoff: rejected structure birth — inverted/zero range",
                    );
                    return Ok(());
                }
                let h_over_mid = raw / mid;
                if h_over_mid > cap {
                    tracing::info!(
                        symbol = %symbol,
                        interval = %interval,
                        h_over_mid = %format!("{h_over_mid:.3}"),
                        cap = %format!("{cap:.3}"),
                        event = %wy_event.as_str(),
                        range_bottom = %tracker.range_bottom,
                        range_top = %tracker.range_top,
                        "wyckoff: rejected structure birth — range too wide for TF",
                    );
                    return Ok(());
                }
            }

            let events_json = serde_json::to_value(&tracker.events)?;
            let segment = "futures"; // default, TODO: from engine_symbol
            insert_wyckoff_structure(
                pool,
                &WyckoffStructureInsert {
                    symbol,
                    interval,
                    exchange,
                    segment,
                    schematic: schematic.as_str(),
                    current_phase: tracker.current_phase.as_str(),
                    range_top: tracker.range_top,
                    range_bottom: tracker.range_bottom,
                    creek_level: tracker.creek,
                    ice_level: tracker.ice,
                    events_json,
                    confidence: tracker.confidence(),
                },
            )
            .await?;
        }
    }
    Ok(())
}

/// P28a — Resolve the per-TF Wyckoff structure TTL in seconds from
/// `system_config`. Key path: `wyckoff.structure.ttl_seconds.<TF>` with
/// a `wyckoff.structure.ttl_seconds.default` fallback. Values are
/// clamped to [60s, 365d] to avoid operator typos disabling the guard.
async fn resolve_wyckoff_ttl_seconds(pool: &sqlx::PgPool, interval: &str) -> u64 {
    let key = format!("structure.ttl_seconds.{interval}");
    let env_key = format!("QTSS_WYCKOFF_TTL_{}", interval.to_uppercase());
    // resolve_system_u64 falls back to the provided default when the
    // key is missing; we then treat 0 as "use default fallback key".
    let per_tf = qtss_storage::resolve_system_u64(
        pool, "wyckoff", &key, &env_key, 0, 60, 31_536_000,
    )
    .await;
    if per_tf > 0 {
        return per_tf;
    }
    qtss_storage::resolve_system_u64(
        pool,
        "wyckoff",
        "structure.ttl_seconds.default",
        "QTSS_WYCKOFF_TTL_DEFAULT",
        4_320_000, // ~50 days
        60,
        31_536_000,
    )
    .await
}

/// Synthetic Phase E trigger.
///
/// No detector emits Markup / Markdown and JAC / BreakOfIce are rare
/// in real data, so Phase D structures never reach completion on their
/// own. When the tracker is in Phase D we inspect the most recent
/// window of the chronological feed: if a sustained breakout matching
/// the schematic's directional bias is confirmed (>=60% of the last
/// `N` closes beyond range ± 0.5%), inject a single synthetic
/// Markup / Markdown event. The tracker's own phase derivation then
/// promotes `current_phase` to E and the caller marks the row complete.
///
/// Source is tagged via score (0.55 — below detector norms) so analysts
/// can filter these out if they want to audit only hard-detector events.
/// Faz 8.2 — Markup/Markdown injection tunables (CLAUDE.md #2).
/// Previously hardcoded (30 / 10 / 60% / 0.5%); now config-driven so
/// operators can relax or tighten the synthetic Phase-E trigger without
/// a deploy.
struct MarkupInjectConfig {
    window_bars: usize,
    min_window_bars: usize,
    confirm_pct: f64,
    breakout_tol_pct: f64,
}

async fn resolve_markup_inject_config(pool: &sqlx::PgPool) -> MarkupInjectConfig {
    MarkupInjectConfig {
        window_bars: qtss_storage::resolve_system_u64(
            pool, "wyckoff", "markup_inject.window_bars",
            "QTSS_WYCKOFF_MARKUP_WINDOW_BARS", 30, 1, 1000,
        ).await as usize,
        min_window_bars: qtss_storage::resolve_system_u64(
            pool, "wyckoff", "markup_inject.min_window_bars",
            "QTSS_WYCKOFF_MARKUP_MIN_WINDOW_BARS", 10, 1, 1000,
        ).await as usize,
        confirm_pct: qtss_storage::resolve_system_f64(
            pool, "wyckoff", "markup_inject.confirm_pct",
            "QTSS_WYCKOFF_MARKUP_CONFIRM_PCT", 60.0,
        ).await,
        breakout_tol_pct: qtss_storage::resolve_system_f64(
            pool, "wyckoff", "markup_inject.breakout_tol_pct",
            "QTSS_WYCKOFF_MARKUP_BREAKOUT_TOL_PCT", 0.5,
        ).await,
    }
}

fn maybe_inject_markup_markdown(
    tracker: &mut qtss_wyckoff::WyckoffStructureTracker,
    chronological: &[MarketBarRow],
    time_ms: Option<i64>,
    bar_idx: u64,
    cfg: &MarkupInjectConfig,
) {
    use qtss_wyckoff::{WyckoffEvent, WyckoffSchematic};

    // Already have a terminal event — nothing to inject.
    if tracker.events.iter().any(|e| {
        matches!(e.event, WyckoffEvent::Markup | WyckoffEvent::Markdown)
    }) {
        return;
    }

    let bullish = matches!(
        tracker.schematic,
        WyckoffSchematic::Accumulation | WyckoffSchematic::ReAccumulation
    );
    let (top, bot) = (tracker.range_top, tracker.range_bottom);
    if top <= 0.0 || bot <= 0.0 || top <= bot {
        return;
    }
    let tol = cfg.breakout_tol_pct / 100.0;
    let threshold = if bullish { top * (1.0 + tol) } else { bot * (1.0 - tol) };

    // Window ending at bar_idx (inclusive).
    let end = (bar_idx as usize + 1).min(chronological.len());
    let start = end.saturating_sub(cfg.window_bars);
    let window = &chronological[start..end];
    if window.len() < cfg.min_window_bars {
        return;
    }

    let confirmed = window
        .iter()
        .filter(|r| {
            let c = r.close.to_f64().unwrap_or(0.0);
            if bullish { c > threshold } else { c < threshold }
        })
        .count();
    // Require ≥ confirm_pct of recent bars beyond the breakout
    // threshold — one spike is not a markup.
    let ratio = (confirmed as f64) / (window.len() as f64) * 100.0;
    if ratio < cfg.confirm_pct {
        return;
    }

    let last = match window.last() {
        Some(r) => r,
        None => return,
    };
    let price = last.close.to_f64().unwrap_or(0.0);
    let event = if bullish { WyckoffEvent::Markup } else { WyckoffEvent::Markdown };
    let ev_time_ms = time_ms.or_else(|| Some(last.open_time.timestamp_millis()));
    tracker.record_event_with_time(event, bar_idx, price, 0.55, ev_time_ms);
}

/// Elliott Deep: insert wave segments into `wave_chain` and link the
/// cross-TF matryoshka hierarchy (parent ↔ children).
async fn link_elliott_to_wave_chain(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    interval: &str,
    timeframe: Timeframe,
    detection: &Detection,
    detection_id: Uuid,
    chronological: &[MarketBarRow],
) -> anyhow::Result<()> {
    use qtss_domain::v2::detection::WaveDegree;
    use qtss_storage::wave_chain::{
        adopt_children, find_by_detection, find_parent_wave, insert_wave_chain,
        WaveChainInsert,
    };

    // Dedup: if this detection already has wave_chain rows, skip
    if let Ok(Some(_)) = find_by_detection(pool, detection_id).await {
        return Ok(());
    }

    // Dedup: if there's already an active wave_chain with the same
    // (exchange, symbol, timeframe, subkind), skip to avoid duplicates
    // from the orchestrator re-detecting the same pattern each tick.
    {
        let subkind_str = match &detection.kind {
            PatternKind::Elliott(s) => s.as_str(),
            _ => "",
        };
        let existing: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM wave_chain WHERE exchange=$1 AND symbol=$2 AND timeframe=$3 AND subkind=$4 AND state='active' LIMIT 1"
        )
        .bind(exchange).bind(symbol).bind(interval).bind(subkind_str)
        .fetch_optional(pool).await?;
        if existing.is_some() {
            return Ok(());
        }
    }

    let degree = WaveDegree::from_timeframe(timeframe);
    let subkind = match &detection.kind {
        PatternKind::Elliott(s) => s.clone(),
        _ => return Ok(()),
    };
    let is_impulse = subkind.contains("impulse") || subkind.contains("diagonal");
    let kind = if is_impulse { "impulse" } else { "corrective" };
    // Combination and triangle already carry correct labels (W-A, W-B, X/Y,
    // Y-A, etc. or A, B, C, D, E). Only apply degree notation to simple
    // impulse/zigzag/flat where anchors use raw numbers or A/B/C.
    let use_own_labels = subkind.contains("combination")
        || subkind.contains("triangle");
    let notation: &[&str] = if use_own_labels {
        &[] // empty → always fall through to anchor's own label
    } else if is_impulse {
        degree.impulse_notation().as_slice()
    } else {
        degree.corrective_notation().as_slice()
    };

    // Build segment data first (time ranges needed for parent lookup)
    struct SegData {
        label: Option<String>,
        time_start: Option<chrono::DateTime<chrono::Utc>>,
        time_end: Option<chrono::DateTime<chrono::Utc>>,
        price_start: Decimal,
        price_end: Decimal,
        bar_start: i64,
        bar_end: i64,
        direction: String,
    }

    let anchors = &detection.anchors;
    let mut segments: Vec<SegData> = Vec::new();
    for (i, pair) in anchors.windows(2).enumerate() {
        let a = &pair[0];
        let b = &pair[1];
        let label = if i < notation.len() {
            Some(notation[i].to_string())
        } else {
            a.label.clone().or_else(|| Some(format!("{}", i + 1)))
        };
        let ts = chronological.get(a.bar_index as usize).map(|r| r.open_time);
        let te = chronological.get(b.bar_index as usize).map(|r| r.open_time);
        let dir = if b.price >= a.price { "bullish" } else { "bearish" };
        segments.push(SegData {
            label,
            time_start: ts,
            time_end: te,
            price_start: a.price,
            price_end: b.price,
            bar_start: a.bar_index as i64,
            bar_end: b.bar_index as i64,
            direction: dir.to_string(),
        });
    }

    if segments.is_empty() {
        return Ok(());
    }

    // Per-segment parent lookup: each segment finds its own parent wave
    // on higher TFs. This is correct because a wide detection (e.g., WXY
    // combination spanning months) may have segments falling under different
    // parent waves on the higher TF.

    let parent_tfs = [
        (Timeframe::Mn1, WaveDegree::Supercycle),
        (Timeframe::W1,  WaveDegree::Cycle),
        (Timeframe::D1,  WaveDegree::Primary),
        (Timeframe::H4,  WaveDegree::Intermediate),
        (Timeframe::H1,  WaveDegree::Minor),
        (Timeframe::M30, WaveDegree::Minute),
        (Timeframe::M15, WaveDegree::Minute),
        (Timeframe::M5,  WaveDegree::Minuette),
    ];
    let cur_rank = degree.rank();

    let mut inserted_ids: Vec<(Uuid, Option<chrono::DateTime<chrono::Utc>>, Option<chrono::DateTime<chrono::Utc>>)> = Vec::new();

    for (i, seg) in segments.iter().enumerate() {
        // Each segment independently finds its parent
        let parent_id = if let (Some(ts), Some(te)) = (seg.time_start, seg.time_end) {
            let mut found = None;
            for (ptf, pdeg) in &parent_tfs {
                if pdeg.rank() <= cur_rank { continue; }
                let ptf_str = timeframe_to_interval(*ptf);
                let pdeg_str = pdeg.label();
                if let Ok(Some(row)) = find_parent_wave(pool, exchange, symbol, &ptf_str, pdeg_str, ts, te).await {
                    found = Some(row.id);
                    break;
                }
            }
            found
        } else {
            None
        };

        let row = WaveChainInsert {
            parent_id,
            exchange: exchange.to_string(),
            symbol: symbol.to_string(),
            timeframe: interval.to_string(),
            degree: degree.label().to_string(),
            kind: kind.to_string(),
            direction: seg.direction.clone(),
            wave_number: seg.label.clone(),
            bar_start: seg.bar_start,
            bar_end: seg.bar_end,
            price_start: seg.price_start,
            price_end: seg.price_end,
            structural_score: detection.structural_score,
            state: "active".to_string(),
            detection_id: Some(detection_id),
            time_start: seg.time_start,
            time_end: seg.time_end,
            subkind: subkind.clone(),
        };

        match insert_wave_chain(pool, &row).await {
            Ok(id) => inserted_ids.push((id, seg.time_start, seg.time_end)),
            Err(e) => {
                tracing::warn!(%e, "wave_chain insert failed for segment {i}");
            }
        }
    }

    // Adopt orphan children: search ALL lower TFs, not just the fixed child.
    // A wave on 1W might have sub-waves on 1D, 4H, or even 1H depending on
    // its duration. We try each lower TF+degree pair.
    {
        let all_tfs = [
            (Timeframe::W1,  WaveDegree::Cycle),
            (Timeframe::D1,  WaveDegree::Primary),
            (Timeframe::H4,  WaveDegree::Intermediate),
            (Timeframe::H1,  WaveDegree::Minor),
            (Timeframe::M30, WaveDegree::Minute),
            (Timeframe::M15, WaveDegree::Minute),
            (Timeframe::M5,  WaveDegree::Minuette),
            (Timeframe::M1,  WaveDegree::Subminuette),
        ];
        // Only adopt from TFs strictly lower than current
        let cur_rank = degree.rank();
        for (child_tf, child_deg) in &all_tfs {
            if child_deg.rank() >= cur_rank { continue; }
            let ctf_str = timeframe_to_interval(*child_tf);
            let cdeg_str = child_deg.label();
            for &(seg_id, ts, te) in &inserted_ids {
                if let (Some(ts), Some(te)) = (ts, te) {
                    let _ = adopt_children(pool, seg_id, exchange, symbol, &ctf_str, cdeg_str, ts, te).await;
                }
            }
        }
    }

    let linked_parents = inserted_ids.iter().filter(|(_, _, _)| true).count();
    tracing::info!(
        symbol,
        interval,
        degree = degree.label(),
        segments = inserted_ids.len(),
        "wave_chain linked ({linked_parents} segments)"
    );

    // ── Projection generation ──
    // Use the last segment as the source wave for projections.
    if let Some(&(last_seg_id, last_ts, _last_te)) = inserted_ids.last() {
        let prices: Vec<f64> = detection.anchors.iter()
            .map(|a| a.price.to_f64().unwrap_or(0.0))
            .collect();
        let avg_spacing = if detection.anchors.len() >= 2 {
            let span = detection.anchors.last().unwrap().bar_index
                     - detection.anchors.first().unwrap().bar_index;
            (span / (detection.anchors.len() as u64 - 1).max(1)).max(1)
        } else { 1 };

        let _ = crate::v2_projection_loop::generate_projections_for_wave(
            pool,
            last_seg_id,
            exchange,
            symbol,
            interval,
            degree.label(),
            &subkind,
            &prices,
            avg_spacing,
            segments.last().and_then(|s| s.label.as_deref()),
            None, // TODO: fetch sibling W2 kind for alternation
            last_ts,
        ).await;
    }

    Ok(())
}

/// Look up the most recent open detection for this (symbol, tf, family,
/// subkind) and return its id if its last anchor matches — meaning the
/// new detection is the same structure. The caller can then update the
/// existing row's projection instead of inserting a duplicate.
async fn dedup_open(
    repo: &V2DetectionRepository,
    exchange: &str,
    symbol: &str,
    timeframe: &str,
    family: &str,
    _subkind: &str,
    last_anchor_idx: u64,
) -> anyhow::Result<Option<Uuid>> {
    let rows = repo
        .list_filtered(DetectionFilter {
            exchange: Some(exchange),
            symbol: Some(symbol),
            timeframe: Some(timeframe),
            family: Some(family),
            state: Some("forming"),
            mode: None,
            limit: 5,
        })
        .await?;

    for row in rows {
        if let Some(idx) = row
            .raw_meta
            .get("last_anchor_idx")
            .and_then(|v| v.as_u64())
        {
            if idx == last_anchor_idx {
                return Ok(Some(row.id));
            }
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

/// Convert a Timeframe back to TradingView-style interval string
/// (inverse of parse_timeframe).
fn timeframe_to_interval(tf: Timeframe) -> &'static str {
    match tf {
        Timeframe::M1 => "1m",
        Timeframe::M3 => "3m",
        Timeframe::M5 => "5m",
        Timeframe::M15 => "15m",
        Timeframe::M30 => "30m",
        Timeframe::H1 => "1h",
        Timeframe::H2 => "2h",
        Timeframe::H4 => "4h",
        Timeframe::H6 => "6h",
        Timeframe::H8 => "8h",
        Timeframe::H12 => "12h",
        Timeframe::D1 => "1d",
        Timeframe::D3 => "3d",
        Timeframe::W1 => "1w",
        Timeframe::Mn1 => "1mo",
    }
}

pub(crate) fn parse_timeframe(interval: &str) -> Option<Timeframe> {
    // engine_symbols stores TradingView-ish strings ("1m", "1h", "1d");
    // Timeframe::from_str expects "m1"/"h1"/"d1". Translate.
    let s = interval.trim().to_lowercase();
    let tf = match s.as_str() {
        "1m" => Timeframe::M1,
        "3m" => Timeframe::M3,
        "5m" => Timeframe::M5,
        "15m" => Timeframe::M15,
        "30m" => Timeframe::M30,
        "1h" => Timeframe::H1,
        "2h" => Timeframe::H2,
        "4h" => Timeframe::H4,
        "6h" => Timeframe::H6,
        "8h" => Timeframe::H8,
        "12h" => Timeframe::H12,
        "1d" => Timeframe::D1,
        "3d" => Timeframe::D3,
        "1w" => Timeframe::W1,
        "1mo" | "1mn" => Timeframe::Mn1,
        _ => return None,
    };
    Some(tf)
}

pub(crate) fn build_instrument(exchange: &str, segment: &str, symbol: &str) -> Instrument {
    let venue = parse_venue(exchange);
    let asset_class = parse_asset_class(exchange, segment);
    Instrument {
        venue,
        asset_class,
        symbol: symbol.to_string(),
        // Quote currency / tick / lot are not strictly needed by the
        // pure detectors so we use neutral placeholders. The chart
        // endpoint reads symbol+venue from the row, not the instrument
        // serialization.
        quote_ccy: "USDT".into(),
        tick_size: rust_decimal::Decimal::new(1, 8),
        lot_size: rust_decimal::Decimal::new(1, 8),
        session: SessionCalendar::binance_24x7(),
    }
}

fn parse_venue(exchange: &str) -> Venue {
    match exchange.trim().to_lowercase().as_str() {
        "binance" => Venue::Binance,
        "bybit" => Venue::Bybit,
        "okx" => Venue::Okx,
        "bist" => Venue::Bist,
        "nasdaq" => Venue::Nasdaq,
        "nyse" => Venue::Nyse,
        other => Venue::Custom(other.to_string()),
    }
}

fn parse_asset_class(exchange: &str, segment: &str) -> AssetClass {
    let ex = exchange.trim().to_lowercase();
    let seg = segment.trim().to_lowercase();
    match (ex.as_str(), seg.as_str()) {
        ("binance", "futures") | ("binance", "usdm") | ("binance", "coinm") => {
            AssetClass::CryptoFutures
        }
        ("binance", _) | ("bybit", _) | ("okx", _) => AssetClass::CryptoSpot,
        ("bist", _) => AssetClass::EquityBist,
        ("nasdaq", _) => AssetClass::EquityNasdaq,
        ("nyse", _) => AssetClass::EquityNyse,
        _ => AssetClass::CryptoSpot,
    }
}

/// Direction inferred from a detection's `family + subkind` so the
/// price-breach sweep knows which side of `invalidation_price` is the
/// "fail" side. Each family encodes direction differently — the table
/// below is the single source of truth (CLAUDE.md #1: lookup over
/// scattered if/else).
#[derive(Debug, Clone, Copy)]
enum Direction {
    Long,
    Short,
}

fn infer_direction(_family: &str, subkind: &str) -> Option<Direction> {
    let s = subkind.to_ascii_lowercase();
    // Long-biased markers across all families.
    const LONG_MARKERS: &[&str] = &[
        "bull", "long", "bottom", "spring", "accumulation",
        "ascending", "inverse_head", "cup_handle",
    ];
    // Short-biased markers.
    const SHORT_MARKERS: &[&str] = &[
        "bear", "short", "top", "upthrust", "distribution",
        "descending", "head_shoulders", "head_and_shoulders",
    ];
    // SHORT must be checked first because "head_shoulders" is short
    // while "inverse_head_shoulders" (long) also contains "head_shoulders".
    // The inverse case is caught by LONG_MARKERS via "inverse_head".
    if s.contains("inverse_head") || s.contains("inv_head") {
        return Some(Direction::Long);
    }
    for m in SHORT_MARKERS {
        if s.contains(m) {
            return Some(Direction::Short);
        }
    }
    for m in LONG_MARKERS {
        if s.contains(m) {
            return Some(Direction::Long);
        }
    }
    None
}

pub(crate) fn split_pattern_kind(kind: &PatternKind) -> (&'static str, &str) {
    match kind {
        PatternKind::Elliott(s) => ("elliott", s.as_str()),
        PatternKind::Harmonic(s) => ("harmonic", s.as_str()),
        PatternKind::Classical(s) => ("classical", s.as_str()),
        PatternKind::Wyckoff(s) => ("wyckoff", s.as_str()),
        PatternKind::Range(s) => ("range", s.as_str()),
        PatternKind::Gap(s) => ("gap", s.as_str()),
        PatternKind::Candle(s) => ("candle", s.as_str()),
        PatternKind::Custom(s) => ("custom", s.as_str()),
    }
}

/// Backfill projections for wave_chain entries missing projections.
/// Runs once per tick, processes a limited batch to avoid overload.
async fn backfill_projections(pool: &PgPool) -> anyhow::Result<()> {
    use qtss_storage::wave_chain::WaveChainRow;

    // Find last segments of each detection that have no projections yet.
    let candidates: Vec<WaveChainRow> = sqlx::query_as(
        r#"SELECT wc.* FROM wave_chain wc
           INNER JOIN (
             SELECT detection_id, MAX(time_end) AS max_te
             FROM wave_chain
             WHERE detection_id IS NOT NULL AND state != 'invalidated'
             GROUP BY detection_id
           ) last ON wc.detection_id = last.detection_id AND wc.time_end = last.max_te
           LEFT JOIN wave_projections wp ON wp.source_wave_id = wc.id
           WHERE wp.id IS NULL
           LIMIT 50"#,
    )
    .fetch_all(pool)
    .await?;

    if candidates.is_empty() {
        return Ok(());
    }

    let mut generated = 0usize;
    for wc in &candidates {
        let detection_id = match wc.detection_id {
            Some(d) => d,
            None => continue,
        };

        // Fetch all segments of this detection to reconstruct anchor prices
        let segs: Vec<WaveChainRow> = sqlx::query_as(
            r#"SELECT * FROM wave_chain
               WHERE detection_id = $1 ORDER BY time_start ASC"#,
        ).bind(detection_id).fetch_all(pool).await?;

        if segs.is_empty() { continue; }

        // Reconstruct: [first.price_start, first.price_end, second.price_end, ...]
        let mut prices = vec![segs[0].price_start.to_f64().unwrap_or(0.0)];
        for s in &segs {
            prices.push(s.price_end.to_f64().unwrap_or(0.0));
        }

        let total_bars = segs.last().map(|s| s.bar_end).unwrap_or(0)
                       - segs.first().map(|s| s.bar_start).unwrap_or(0);
        let avg_spacing = if segs.len() > 1 {
            (total_bars as u64 / segs.len() as u64).max(1)
        } else { 1 };

        let count = crate::v2_projection_loop::generate_projections_for_wave(
            pool,
            wc.id,
            &wc.exchange,
            &wc.symbol,
            &wc.timeframe,
            &wc.degree,
            &wc.subkind,
            &prices,
            avg_spacing,
            None,
            None,
            wc.time_end,
        ).await?;
        generated += count;
    }

    if generated > 0 {
        info!(generated, "projection backfill completed");
    }

    Ok(())
}

/// Validate active projections: fetch distinct (exchange, symbol, tf) combos
/// from active projections, get latest price for each, and run validation.
async fn validate_active_projections(pool: &PgPool) -> anyhow::Result<()> {
    // Get unique series with active projections
    let series: Vec<(String, String, String)> = sqlx::query_as(
        r#"SELECT DISTINCT exchange, symbol, timeframe
           FROM wave_projections
           WHERE state IN ('active', 'leading')
           LIMIT 100"#,
    )
    .fetch_all(pool)
    .await?;

    if series.is_empty() {
        return Ok(());
    }

    for (exchange, symbol, timeframe) in &series {
        // Get latest close price from market_bars
        let latest: Option<(Decimal, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
            r#"SELECT close, open_time FROM market_bars
               WHERE LOWER(BTRIM(exchange)) = LOWER(BTRIM($1))
                 AND BTRIM(symbol) = BTRIM($2)
                 AND BTRIM(interval) = BTRIM($3)
               ORDER BY open_time DESC LIMIT 1"#,
        )
        .bind(exchange)
        .bind(symbol)
        .bind(timeframe)
        .fetch_optional(pool)
        .await?;

        if let Some((price, time)) = latest {
            let price_f = price.to_f64().unwrap_or(0.0);
            if let Err(e) = crate::v2_projection_loop::validate_projections(
                pool, exchange, symbol, timeframe, price_f, time,
            ).await {
                warn!(%e, symbol, timeframe, "projection validation error");
            }
        }
    }

    Ok(())
}
