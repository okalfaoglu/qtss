//! GapConfig — thresholds for gap classification. All fields are seeded
//! from `system_config` rows in migration 0158; no hard-coded sabit
//! (CLAUDE.md #2). Defaults below are fallback-only.

use crate::error::{GapError, GapResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapConfig {
    /// Minimum gap size as fraction of previous close (e.g. 0.005 = 0.5%).
    pub min_gap_pct: f64,
    /// Moving-average lookback (bars) for volume baseline.
    pub volume_sma_bars: usize,
    /// Minimum volume multiplier for "breakaway" classification.
    pub vol_mult_breakaway: f64,
    /// Minimum volume multiplier for "runaway" classification.
    pub vol_mult_runaway: f64,
    /// Minimum volume multiplier for "exhaustion" classification.
    pub vol_mult_exhaustion: f64,
    /// Consolidation-flat detection: max relative (high-low)/mid average
    /// across `consolidation_lookback` bars before a breakaway gap.
    pub range_flat_pct: f64,
    /// Lookback bars used to assess pre-gap consolidation.
    pub consolidation_lookback: usize,
    /// Runaway: minimum consecutive same-sign bars of prior trend.
    pub runaway_trend_bars: usize,
    /// Runaway: minimum cumulative return over `runaway_trend_bars`.
    pub runaway_trend_min_pct: f64,
    /// Exhaustion: within how many bars following the gap a reversal
    /// closing below pre-gap close (bull) / above (bear) is required.
    pub exhaustion_reversal_bars: usize,
    /// Island reversal: max bars between the two opposing gaps.
    pub island_max_bars: usize,
    /// Minimum structural score to emit a detection.
    pub min_structural_score: f32,
}

impl Default for GapConfig {
    fn default() -> Self {
        Self {
            min_gap_pct: 0.005,
            volume_sma_bars: 20,
            vol_mult_breakaway: 1.5,
            vol_mult_runaway: 1.3,
            vol_mult_exhaustion: 1.8,
            range_flat_pct: 0.02,
            consolidation_lookback: 10,
            runaway_trend_bars: 5,
            runaway_trend_min_pct: 0.02,
            exhaustion_reversal_bars: 5,
            island_max_bars: 10,
            min_structural_score: 0.5,
        }
    }
}

impl GapConfig {
    pub fn validate(&self) -> GapResult<()> {
        if self.min_gap_pct <= 0.0 {
            return Err(GapError::InvalidConfig("min_gap_pct must be > 0".into()));
        }
        if self.volume_sma_bars < 2 {
            return Err(GapError::InvalidConfig("volume_sma_bars must be >= 2".into()));
        }
        if self.consolidation_lookback < 3 {
            return Err(GapError::InvalidConfig(
                "consolidation_lookback must be >= 3".into(),
            ));
        }
        if self.runaway_trend_bars < 2 {
            return Err(GapError::InvalidConfig(
                "runaway_trend_bars must be >= 2".into(),
            ));
        }
        if self.island_max_bars < 1 {
            return Err(GapError::InvalidConfig(
                "island_max_bars must be >= 1".into(),
            ));
        }
        Ok(())
    }
}
