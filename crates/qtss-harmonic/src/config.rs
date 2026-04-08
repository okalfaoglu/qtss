//! Detector configuration.

use crate::error::{HarmonicError, HarmonicResult};
use qtss_domain::v2::pivot::PivotLevel;

#[derive(Debug, Clone)]
pub struct HarmonicConfig {
    /// Which pivot level the detector consumes.
    pub pivot_level: PivotLevel,
    /// Minimum structural score (0..1) below which a candidate is dropped.
    pub min_structural_score: f32,
    /// Allowed deviation around each ratio range edge, as a fraction.
    /// 0.05 = ratios up to 5% outside the canonical range still pass.
    /// Per-spec ranges in `patterns.rs` already include realistic
    /// tolerance — this is an extra global slack the operator can tune.
    pub global_slack: f64,
}

impl HarmonicConfig {
    pub fn defaults() -> Self {
        Self {
            pivot_level: PivotLevel::L1,
            min_structural_score: 0.50,
            global_slack: 0.0,
        }
    }

    pub fn validate(&self) -> HarmonicResult<()> {
        if !(0.0..=1.0).contains(&(self.min_structural_score as f64)) {
            return Err(HarmonicError::InvalidConfig(
                "min_structural_score must be in 0..=1".into(),
            ));
        }
        if !(0.0..=0.5).contains(&self.global_slack) {
            return Err(HarmonicError::InvalidConfig(
                "global_slack must be in 0..=0.5".into(),
            ));
        }
        Ok(())
    }
}
