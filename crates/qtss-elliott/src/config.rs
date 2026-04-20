//! Detector configuration.
//!
//! Mirrors the seed migration 0016 keys (`detection.elliott.*`). The
//! crate itself never reads from `qtss-config` — the caller resolves
//! the values and constructs this struct so the detector stays pure.

use crate::error::{ElliottError, ElliottResult};
use qtss_domain::v2::pivot::PivotLevel;

#[derive(Debug, Clone)]
pub struct ElliottConfig {
    // Clone needed: aggregator.rs builds N formation detectors from a
    // single base config and each takes ownership.
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
            // Faz 14.A14 — lowered from 0.60 to 0.45. 0.60 silenced
            // meşru impulses whose fib ratios sat in the ~10-15%
            // tolerance band (eg. wave-2 retrace 0.47 vs 0.5). The
            // validator still gets to re-check with its own threshold
            // downstream, so being more permissive at the detector
            // layer produces more candidates, not more false positives.
            min_structural_score: 0.45,
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
