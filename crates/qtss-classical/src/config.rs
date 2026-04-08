//! Detector configuration.

use crate::error::{ClassicalError, ClassicalResult};
use qtss_domain::v2::pivot::PivotLevel;

#[derive(Debug, Clone)]
pub struct ClassicalConfig {
    /// Which pivot level to consume.
    pub pivot_level: PivotLevel,
    /// Drop candidates whose structural score falls under this floor.
    pub min_structural_score: f32,
    /// Maximum allowed relative deviation between the two "equal" peaks
    /// of a double-top / shoulders / triangle base, expressed as a
    /// fraction of the price (0.03 = 3%).
    pub equality_tolerance: f64,
    /// Triangle convergence: the apex (intersection of upper/lower
    /// trendlines) must be within `apex_horizon` future bars from the
    /// last pivot, otherwise the pattern is too loose.
    pub apex_horizon_bars: u64,
}

impl ClassicalConfig {
    pub fn defaults() -> Self {
        Self {
            pivot_level: PivotLevel::L1,
            min_structural_score: 0.50,
            equality_tolerance: 0.03,
            apex_horizon_bars: 50,
        }
    }

    pub fn validate(&self) -> ClassicalResult<()> {
        if !(0.0..=1.0).contains(&(self.min_structural_score as f64)) {
            return Err(ClassicalError::InvalidConfig(
                "min_structural_score must be in 0..=1".into(),
            ));
        }
        if !(0.0..=0.25).contains(&self.equality_tolerance) {
            return Err(ClassicalError::InvalidConfig(
                "equality_tolerance must be in 0..=0.25".into(),
            ));
        }
        if self.apex_horizon_bars == 0 {
            return Err(ClassicalError::InvalidConfig(
                "apex_horizon_bars must be > 0".into(),
            ));
        }
        Ok(())
    }
}
