//! Target engine configuration.

use crate::error::{TargetEngineError, TargetEngineResult};

#[derive(Debug, Clone)]
pub struct TargetEngineConfig {
    /// Two targets within this fraction of price are merged into one
    /// cluster (e.g. 0.005 = 0.5%). Cluster price is the weight-weighted
    /// mean of the members; cluster weight is `min(1.0, sum_of_weights)`.
    pub cluster_tolerance: f64,
    /// Maximum number of targets to keep after clustering. Lowest-weight
    /// targets are dropped first. Use a high value to disable.
    pub max_targets: usize,
    /// Minimum weight a single target must have to survive trimming.
    pub min_weight: f32,
}

impl TargetEngineConfig {
    pub fn defaults() -> Self {
        Self {
            cluster_tolerance: 0.005,
            max_targets: 5,
            min_weight: 0.05,
        }
    }

    pub fn validate(&self) -> TargetEngineResult<()> {
        if !(0.0..=0.10).contains(&self.cluster_tolerance) {
            return Err(TargetEngineError::InvalidConfig(
                "cluster_tolerance must be in 0..=0.10".into(),
            ));
        }
        if self.max_targets == 0 {
            return Err(TargetEngineError::InvalidConfig(
                "max_targets must be > 0".into(),
            ));
        }
        if !(0.0..=1.0).contains(&(self.min_weight as f64)) {
            return Err(TargetEngineError::InvalidConfig(
                "min_weight must be in 0..=1".into(),
            ));
        }
        Ok(())
    }
}
