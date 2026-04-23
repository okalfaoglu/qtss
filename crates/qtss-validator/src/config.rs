//! Validator thresholds. Per-family knobs kept in one struct so the
//! worker loads a single config bag per tick.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorConfig {
    /// Harmonic PRZ break tolerance as fraction of XA leg. Close
    /// beyond D ± this = invalidate.
    pub harmonic_break_pct: f64,
    /// Range zone invalidation — close through the zone beyond this
    /// fraction of the zone height. 1.0 = fully traversed.
    pub range_full_fill_pct: f64,
    /// Gap close threshold — gap is considered filled when close
    /// crosses this fraction of the original gap magnitude.
    pub gap_close_pct: f64,
    /// Motive wave 1 break tolerance as fraction of wave-1 height.
    /// 0.0 = strict (one bar closing through). 0.005 = 50bps buffer.
    pub motive_wave1_buffer_pct: f64,
    /// SMC event invalidation buffer as fraction of reference_price.
    /// Absorbs a few ticks of noise before flipping.
    pub smc_break_buffer_pct: f64,
    /// ORB re-entry — a break that reverses back inside the OR
    /// within this many bars is invalidated as a fakeout.
    pub orb_reentry_bars: u32,
    /// Generic classical/fallback — close beyond invalidation_price
    /// by this fraction of price.
    pub classical_break_pct: f64,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            harmonic_break_pct: 0.03,
            range_full_fill_pct: 1.0,
            gap_close_pct: 0.95,
            motive_wave1_buffer_pct: 0.005,
            smc_break_buffer_pct: 0.003,
            orb_reentry_bars: 3,
            classical_break_pct: 0.002,
        }
    }
}
