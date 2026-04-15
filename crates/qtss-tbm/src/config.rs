//! TBM runtime configuration. Every tunable lives here so the worker
//! can hydrate it from `system_config` (CLAUDE.md #2 — no hardcoded
//! constants in code paths). The struct is plain data; the loader
//! lives in `qtss-worker` next to the v2 detector binding so this
//! crate stays storage-agnostic.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TbmPillarWeights {
    pub momentum: f64,
    pub volume: f64,
    pub structure: f64,
    pub onchain: f64,
}

impl Default for TbmPillarWeights {
    fn default() -> Self {
        Self {
            momentum: 0.30,
            volume: 0.25,
            structure: 0.30,
            onchain: 0.15,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TbmSetupTuning {
    /// Minimum aggregate score required to emit a setup.
    pub min_score: f64,
    /// Minimum number of pillars whose individual score crosses
    /// `pillar_active_threshold` for the setup to count.
    pub min_active_pillars: usize,
    /// Per-pillar score threshold above which a pillar counts as
    /// "active" for the active-pillars rule.
    pub pillar_active_threshold: f64,
    /// P22 — maximum age (in bars on the same TF) a `forming` TBM
    /// detection may retain before it is auto-invalidated. Without
    /// this, bottom/top setups from weeks ago linger on the chart
    /// because no downstream process ever closes them out. Default
    /// 12 bars — ~12h on H1, ~2 days on 4h, 12 days on 1d.
    pub max_anchor_age_bars: usize,
}

impl Default for TbmSetupTuning {
    fn default() -> Self {
        Self {
            min_score: 50.0,
            min_active_pillars: 2,
            pillar_active_threshold: 20.0,
            max_anchor_age_bars: 12,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TbmMtfTuning {
    /// Minimum count of confirming sibling timeframes required before
    /// a setup is promoted to "confirmed".
    pub required_confirms: usize,
    /// Minimum alignment score (0–1) across the confirming TFs.
    pub min_alignment: f64,
    /// P26 — per-TF parent map for HTF+LTF confluence ("sniper entry").
    /// Example: "15m" → "1h" means a 15m setup looks up 1h TBM state.
    /// Missing entries disable confluence for that TF.
    pub htf_parents: std::collections::HashMap<String, String>,
}

impl Default for TbmMtfTuning {
    fn default() -> Self {
        let mut htf_parents = std::collections::HashMap::new();
        for (k, v) in [
            ("1m", "5m"), ("5m", "15m"), ("15m", "1h"),
            ("1h", "4h"), ("4h", "1d"), ("1d", "1w"),
        ] { htf_parents.insert(k.to_string(), v.to_string()); }
        Self {
            required_confirms: 2,
            min_alignment: 0.5,
            htf_parents,
        }
    }
}

/// P22f — structural anchor selection. Previously we used a plain
/// `argmin(lows) / argmax(highs)` over the last N bars, which picked
/// the deepest bar in the window regardless of whether it was a pivot,
/// whether the market had confirmed it (no bars to its right), whether
/// the bar had any reversal wick, or whether the move ended on climactic
/// volume. Those raw extrema landed on the *current forming* bar far too
/// often — labels appeared on mid-trend candles that never reversed.
///
/// The new anchor picker ranks pivot-low / pivot-high candidates inside
/// the window by a composite of depth, reversal-wick ratio, and volume
/// climax; bars without `min_right_bars` of right-hand confirmation are
/// excluded. Falls back to plain argmin/argmax only if nothing in the
/// window qualifies (early in a series).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TbmAnchorTuning {
    /// Symmetric radius (in bars) a candidate must dominate on both
    /// sides to count as a pivot extremum.
    pub pivot_radius: usize,
    /// Minimum completed bars AFTER the candidate before it is eligible
    /// as an anchor. Keeps the picker off the forming bar.
    pub min_right_bars: usize,
    /// Candidate must have lower/upper wick at least this fraction of
    /// its total range. Weeds out mid-trend low-wick bars.
    pub wick_min_ratio: f64,
    /// Volume climax bonus kicks in when bar volume ≥ this × 20-bar
    /// average. 1.0 = "average or above".
    pub vol_min_ratio: f64,
    /// When true, a candidate MUST take out a prior window extreme
    /// (fake breakdown / fake breakout) to be considered — pure
    /// Wyckoff-style gating. Default false: sweep remains a weighted
    /// bonus term so V-bottom reversals without a liquidity grab are
    /// still picked up.
    pub sweep_required: bool,
    /// P22g — tolerance (fraction of price) within which a prior pivot
    /// counts as an "equal low / equal high" touch of the candidate.
    /// 0.002 = 0.2%. Tighter values demand cleaner double/triple
    /// bottoms; looser values pick up wider liquidity pools.
    pub equal_level_tol: f64,
    /// P22g — minimum number of prior pivot touches (NOT counting the
    /// candidate itself) at roughly the candidate's price for the
    /// equal-level bonus to fire. 1 = double bottom/top; 2 = triple.
    pub equal_level_min_touches: usize,
    /// P22g — when true, a candidate without the required equal-level
    /// touches is rejected outright. Default false: equal-level stays
    /// a scoring bonus; flip for pure-Wyckoff liquidity-pool mode.
    pub equal_level_required: bool,
}

impl Default for TbmAnchorTuning {
    fn default() -> Self {
        Self {
            pivot_radius: 3,
            min_right_bars: 3,
            wick_min_ratio: 0.25,
            vol_min_ratio: 1.0,
            sweep_required: false,
            equal_level_tol: 0.002,
            equal_level_min_touches: 1,
            equal_level_required: false,
        }
    }
}

/// P23 — confirmation state machine. A forming TBM detection doesn't
/// become `confirmed` until the market proves the reversal: price must
/// break the structural level that stood on the opposite side of the
/// anchor (BoS — break of structure) AND sustain that break with a
/// follow-through bar of at least `followthrough_atr_mult × ATR(14)`
/// counter-trend close within `followthrough_bars` bars of the break.
/// Without both legs a forming row ages out to `invalidated(timeout)`
/// after `window_bars` bars.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TbmConfirmTuning {
    /// Master switch — when false, forming rows never auto-promote
    /// and the old pipeline (validator-driven confirm) is used.
    pub bos_required: bool,
    /// Bars to wait after the anchor for BoS to print before we
    /// time the detection out.
    pub window_bars: usize,
    /// Follow-through multiplier on ATR(14) — required closing move
    /// in the reversal direction after the BoS bar.
    pub followthrough_atr_mult: f64,
    /// Bars after BoS to look for the follow-through close.
    pub followthrough_bars: usize,
    /// P23c — maximum bars after the BoS bar within which a retest
    /// (HL for bottom / LH for top) is still considered valid. Past
    /// this point we assume the trend moved on without a textbook
    /// retest and stop hunting.
    pub retest_max_age_bars: usize,
    /// P23c — how close (in ATR units) the retest wick must come to
    /// the broken structural level (pre_hi for bottom, pre_lo for
    /// top) to count as a real retest. 0.5 = "within half an ATR".
    pub retest_proximity_atr: f64,
}

impl Default for TbmConfirmTuning {
    fn default() -> Self {
        Self {
            bos_required: true,
            window_bars: 8,
            followthrough_atr_mult: 1.0,
            followthrough_bars: 3,
            retest_max_age_bars: 12,
            retest_proximity_atr: 0.5,
        }
    }
}

/// P24 — Effort vs Result (Wyckoff volume law) detector. Scans the
/// last `scan_bars` bars around the anchor for textbook volume-mismatch
/// bars and contributes a capped bonus to the volume pillar:
///   * no-supply down-bar (bearish bar on shrinking range + low vol)
///     in a bottom hypothesis → sellers exhausted.
///   * no-demand up-bar (bullish bar on shrinking range + low vol)
///     in a top hypothesis → buyers exhausted.
///   * absorption bar (high vol + small range + close mid-range) →
///     effort without result, classic reversal tell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TbmEffortResultTuning {
    pub enabled: bool,
    /// Trailing bars (ending at the anchor) to scan.
    pub scan_bars: usize,
    /// Range gate — bar total range must be ≤ this × 20-bar avg range
    /// to qualify as "small".
    pub range_small_ratio: f64,
    /// Volume gate for no-supply / no-demand — bar volume must be ≤
    /// this × 20-bar avg.
    pub vol_low_ratio: f64,
    /// Volume gate for absorption — bar volume must be ≥ this × 20-bar
    /// avg.
    pub vol_high_ratio: f64,
    /// Pts added per no-supply / no-demand bar found.
    pub no_supply_demand_pts: f64,
    /// Pts added per absorption bar found.
    pub absorption_pts: f64,
    /// Total bonus cap (so stacked signals don't blow the pillar out).
    pub max_bonus_pts: f64,
}

impl Default for TbmEffortResultTuning {
    fn default() -> Self {
        Self {
            enabled: true,
            scan_bars: 8,
            range_small_ratio: 0.7,
            vol_low_ratio: 0.8,
            vol_high_ratio: 1.5,
            no_supply_demand_pts: 10.0,
            absorption_pts: 15.0,
            max_bonus_pts: 25.0,
        }
    }
}

/// Top-level TBM runtime config. The worker hydrates this from
/// `system_config` once per tick interval; the detector treats it as
/// immutable for the duration of the tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TbmConfig {
    pub enabled: bool,
    pub tick_interval_s: u64,
    pub lookback_bars: usize,
    pub weights: TbmPillarWeights,
    pub setup: TbmSetupTuning,
    pub mtf: TbmMtfTuning,
    pub anchor: TbmAnchorTuning,
    pub confirm: TbmConfirmTuning,
    pub effort_result: TbmEffortResultTuning,
    pub onchain_enabled: bool,
}

impl Default for TbmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tick_interval_s: 60,
            lookback_bars: 300,
            weights: TbmPillarWeights::default(),
            setup: TbmSetupTuning::default(),
            mtf: TbmMtfTuning::default(),
            anchor: TbmAnchorTuning::default(),
            confirm: TbmConfirmTuning::default(),
            effort_result: TbmEffortResultTuning::default(),
            onchain_enabled: false,
        }
    }
}
