//! Regime engine configuration.
//!
//! Defaults mirror migration 0016 keys (`regime.adx_period`,
//! `regime.adx_trend_threshold`, `regime.bb_squeeze_threshold`). The
//! crate itself never reads from `qtss-config` — the caller resolves
//! the values and constructs this struct.

use crate::error::{RegimeError, RegimeResult};

#[derive(Debug, Clone)]
pub struct RegimeConfig {
    /// ADX / DI lookback (Wilder smoothing window).
    pub adx_period: usize,
    /// Bollinger band lookback.
    pub bb_period: usize,
    /// Std-dev multiplier for the bands. 2.0 is the standard.
    pub bb_stddev: f64,
    /// Choppiness Index lookback.
    pub chop_period: usize,
    /// ADX value above which a trending regime is plausible.
    pub adx_trend_threshold: f64,
    /// ADX value above which trend is considered very strong.
    pub adx_strong_threshold: f64,
    /// (band_high - band_low) / mid below which we declare a squeeze.
    pub bb_squeeze_threshold: f64,
    /// ATR/price above which we declare elevated volatility.
    pub volatility_threshold: f64,
    /// Choppiness Index above which we declare a range.
    pub chop_range_threshold: f64,
}

impl RegimeConfig {
    pub fn defaults() -> Self {
        Self {
            adx_period: 14,
            bb_period: 20,
            bb_stddev: 2.0,
            chop_period: 14,
            adx_trend_threshold: 25.0,
            adx_strong_threshold: 40.0,
            bb_squeeze_threshold: 0.05,
            volatility_threshold: 0.04,
            chop_range_threshold: 61.8,
        }
    }

    pub fn validate(&self) -> RegimeResult<()> {
        if self.adx_period < 2 {
            return Err(RegimeError::InvalidConfig("adx_period must be >= 2".into()));
        }
        if self.bb_period < 2 {
            return Err(RegimeError::InvalidConfig("bb_period must be >= 2".into()));
        }
        if self.chop_period < 2 {
            return Err(RegimeError::InvalidConfig(
                "chop_period must be >= 2".into(),
            ));
        }
        if self.bb_stddev <= 0.0 {
            return Err(RegimeError::InvalidConfig("bb_stddev must be > 0".into()));
        }
        if self.adx_strong_threshold <= self.adx_trend_threshold {
            return Err(RegimeError::InvalidConfig(
                "adx_strong_threshold must exceed adx_trend_threshold".into(),
            ));
        }
        Ok(())
    }
}
