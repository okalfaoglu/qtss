//! CandleConfig — thresholds for single/2/3-bar candlestick classification.
//! Seeded from `system_config` via migration 0159 (CLAUDE.md #2).

use crate::error::{CandleError, CandleResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandleConfig {
    /// Doji: body / range <= this threshold counts as a doji body.
    pub doji_body_ratio_max: f64,
    /// Marubozu: (upper_shadow + lower_shadow) / range <= this.
    pub marubozu_shadow_ratio_max: f64,
    /// Hammer / hanging man: lower_shadow / body >= this, upper shadow small.
    pub hammer_lower_shadow_ratio_min: f64,
    /// Hammer: upper_shadow / body <= this (small upper shadow).
    pub hammer_upper_shadow_ratio_max: f64,
    /// Spinning top: body / range <= this and shadows relatively balanced.
    pub spinning_top_body_ratio_max: f64,
    /// Tweezer equality tolerance: |h1-h2|/mid or |l1-l2|/mid.
    pub tweezer_price_tol: f64,
    /// Trend context: number of prior bars to confirm "prior trend"
    /// required by reversal patterns (hanging_man, shooting_star, …).
    pub trend_context_bars: usize,
    /// Minimum cumulative return over `trend_context_bars` for an
    /// established trend (|return| threshold).
    pub trend_context_min_pct: f64,
    /// Minimum structural score for emission.
    pub min_structural_score: f32,
}

impl Default for CandleConfig {
    fn default() -> Self {
        Self {
            doji_body_ratio_max: 0.1,
            marubozu_shadow_ratio_max: 0.05,
            hammer_lower_shadow_ratio_min: 2.0,
            hammer_upper_shadow_ratio_max: 0.5,
            spinning_top_body_ratio_max: 0.3,
            tweezer_price_tol: 0.002,
            trend_context_bars: 5,
            trend_context_min_pct: 0.015,
            min_structural_score: 0.5,
        }
    }
}

impl CandleConfig {
    pub fn validate(&self) -> CandleResult<()> {
        if !(0.0..=1.0).contains(&self.doji_body_ratio_max) {
            return Err(CandleError::InvalidConfig(
                "doji_body_ratio_max must be in [0,1]".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.marubozu_shadow_ratio_max) {
            return Err(CandleError::InvalidConfig(
                "marubozu_shadow_ratio_max must be in [0,1]".into(),
            ));
        }
        if self.hammer_lower_shadow_ratio_min <= 0.0 {
            return Err(CandleError::InvalidConfig(
                "hammer_lower_shadow_ratio_min must be > 0".into(),
            ));
        }
        if self.trend_context_bars < 2 {
            return Err(CandleError::InvalidConfig(
                "trend_context_bars must be >= 2".into(),
            ));
        }
        Ok(())
    }
}
