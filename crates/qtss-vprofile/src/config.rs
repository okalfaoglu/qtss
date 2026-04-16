//! Volume Profile builder configuration.
//!
//! All numbers come from `qtss_config.vprofile.*` at the call site —
//! none of these are hardcoded in the detector / setup builder
//! (CLAUDE.md rule #2).

use crate::error::{VProfileError, VProfileResult};

#[derive(Debug, Clone)]
pub struct VProfileConfig {
    /// Number of price bins between the range low and range high.
    /// Higher → finer resolution but noisier HVN/LVN detection.
    /// Default 50 (Auction Theory texts cite 30–80 as practical band).
    pub bin_count: usize,
    /// Fraction of total volume the Value Area must contain. Standard
    /// auction theory uses 0.70 (1 σ).
    pub value_area_pct: f64,
    /// Minimum prominence of a bin (its volume divided by the local
    /// neighbourhood mean) to qualify as an HVN. Default 1.30 = 30%
    /// above the local average.
    pub hvn_min_prominence_pct: f64,
    /// Maximum ratio of a bin's volume to the local mean below which it
    /// is treated as an LVN ("vacuum"). Default 0.20 = 20% of mean.
    pub lvn_max_pct: f64,
    /// Half-width (in bins) of the local neighbourhood used for
    /// HVN/LVN prominence detection. Default 3 → 7-bin window.
    pub local_neighbourhood_half_width: usize,
    /// How many historical ranges to scan for naked VPOCs (VPOCs that
    /// price has not revisited since they formed). Default 10.
    pub naked_vpoc_lookback_ranges: usize,
    /// Bar count below which a profile is considered too thin to trust
    /// for HVN/LVN extraction. Default 20.
    pub min_bars_for_profile: usize,
}

impl VProfileConfig {
    pub fn defaults() -> Self {
        Self {
            bin_count: 50,
            value_area_pct: 0.70,
            hvn_min_prominence_pct: 1.30,
            lvn_max_pct: 0.20,
            local_neighbourhood_half_width: 3,
            naked_vpoc_lookback_ranges: 10,
            min_bars_for_profile: 20,
        }
    }

    pub fn validate(&self) -> VProfileResult<()> {
        if !(5..=500).contains(&self.bin_count) {
            return Err(VProfileError::InvalidConfig(
                "bin_count must be in 5..=500".into(),
            ));
        }
        if !(0.5..=0.95).contains(&self.value_area_pct) {
            return Err(VProfileError::InvalidConfig(
                "value_area_pct must be in 0.5..=0.95".into(),
            ));
        }
        if self.hvn_min_prominence_pct <= 1.0 {
            return Err(VProfileError::InvalidConfig(
                "hvn_min_prominence_pct must be > 1.0".into(),
            ));
        }
        if !(0.0..1.0).contains(&self.lvn_max_pct) {
            return Err(VProfileError::InvalidConfig(
                "lvn_max_pct must be in 0..1".into(),
            ));
        }
        if self.local_neighbourhood_half_width == 0 {
            return Err(VProfileError::InvalidConfig(
                "local_neighbourhood_half_width must be > 0".into(),
            ));
        }
        if self.min_bars_for_profile == 0 {
            return Err(VProfileError::InvalidConfig(
                "min_bars_for_profile must be > 0".into(),
            ));
        }
        Ok(())
    }
}
