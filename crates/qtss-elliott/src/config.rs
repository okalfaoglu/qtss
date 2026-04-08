//! Detector configuration.
//!
//! Mirrors the seed migration 0016 keys (`detection.elliott.*`). The
//! crate itself never reads from `qtss-config` — the caller resolves
//! the values and constructs this struct so the detector stays pure.

use crate::error::{ElliottError, ElliottResult};
use qtss_domain::v2::pivot::PivotLevel;

#[derive(Debug, Clone)]
pub struct ElliottConfig {
    /// Which pivot level the detector consumes. Detectors declare this
    /// up front so the engine can wire each detector to the right level
    /// of the central PivotTree.
    pub pivot_level: PivotLevel,
    /// Maximum allowed wave 2 retracement of wave 1, as a fraction.
    /// 1.0 = strict Elliott; values < 1.0 reject borderline candidates.
    pub max_wave2_retracement: f64,
    /// Strict overlap check for wave 4 vs wave 1 territory.
    /// `false` would permit diagonals — kept off in this PR.
    pub strict_no_overlap: bool,
    /// Minimum structural score (0..1) below which the detector
    /// suppresses the candidate. Validator may still raise the bar.
    pub min_structural_score: f32,
}

impl ElliottConfig {
    pub fn defaults() -> Self {
        Self {
            pivot_level: PivotLevel::L1,
            max_wave2_retracement: 0.99,
            strict_no_overlap: true,
            min_structural_score: 0.40,
        }
    }

    pub fn validate(&self) -> ElliottResult<()> {
        if !(0.0..=1.0).contains(&self.max_wave2_retracement) {
            return Err(ElliottError::InvalidConfig(
                "max_wave2_retracement must be in 0..=1".into(),
            ));
        }
        if !(0.0..=1.0).contains(&(self.min_structural_score as f64)) {
            return Err(ElliottError::InvalidConfig(
                "min_structural_score must be in 0..=1".into(),
            ));
        }
        Ok(())
    }
}
