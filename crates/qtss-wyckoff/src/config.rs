//! Detector configuration.

use crate::error::{WyckoffError, WyckoffResult};
use qtss_domain::v2::pivot::PivotLevel;

#[derive(Debug, Clone)]
pub struct WyckoffConfig {
    /// Which pivot level to consume.
    pub pivot_level: PivotLevel,
    /// Minimum number of pivots that must lie inside the range box for
    /// it to be considered a valid trading range.
    pub min_range_pivots: usize,
    /// Maximum allowed deviation between the highest highs / lowest lows
    /// of the candidate range, expressed as a fraction of the range
    /// midpoint (0.04 = 4%). Tighter = cleaner box.
    pub range_edge_tolerance: f64,
    /// Volume multiplier (vs. the pivot-volume average) above which a
    /// pivot is considered "climactic" (SC / BC / SOS / SOW signature).
    pub climax_volume_mult: f64,
    /// How far (as a fraction of the range height) a Spring or Upthrust
    /// must penetrate beyond the range boundary before snapping back.
    pub min_penetration: f64,
    /// How far (as a fraction of the range height) a Spring or Upthrust
    /// is allowed to penetrate before being treated as a true breakout
    /// rather than a false break.
    pub max_penetration: f64,
    /// Drop candidates whose structural score falls under this floor.
    pub min_structural_score: f32,
}

impl WyckoffConfig {
    pub fn defaults() -> Self {
        Self {
            pivot_level: PivotLevel::L1,
            min_range_pivots: 5,
            range_edge_tolerance: 0.04,
            climax_volume_mult: 1.8,
            min_penetration: 0.02,
            max_penetration: 0.30,
            min_structural_score: 0.50,
        }
    }

    pub fn validate(&self) -> WyckoffResult<()> {
        if self.min_range_pivots < 4 {
            return Err(WyckoffError::InvalidConfig(
                "min_range_pivots must be >= 4".into(),
            ));
        }
        if !(0.0..=0.25).contains(&self.range_edge_tolerance) {
            return Err(WyckoffError::InvalidConfig(
                "range_edge_tolerance must be in 0..=0.25".into(),
            ));
        }
        if self.climax_volume_mult <= 1.0 {
            return Err(WyckoffError::InvalidConfig(
                "climax_volume_mult must be > 1.0".into(),
            ));
        }
        if !(self.min_penetration < self.max_penetration) {
            return Err(WyckoffError::InvalidConfig(
                "min_penetration must be < max_penetration".into(),
            ));
        }
        if !(0.0..=1.0).contains(&(self.min_structural_score as f64)) {
            return Err(WyckoffError::InvalidConfig(
                "min_structural_score must be in 0..=1".into(),
            ));
        }
        Ok(())
    }
}
