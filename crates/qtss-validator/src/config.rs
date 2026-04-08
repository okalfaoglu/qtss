//! Validator configuration.

use crate::error::{ValidatorError, ValidatorResult};

#[derive(Debug, Clone)]
pub struct ValidatorConfig {
    /// Weight given to the detector's own structural score in the final
    /// blend. The remaining weight (1 - structural_weight) is divided
    /// across the confirmation channels that returned an opinion.
    pub structural_weight: f32,
    /// Per-channel weights inside the confirmation slice. A channel that
    /// is missing here defaults to `1.0`. The combiner normalises across
    /// only the channels that actually voted.
    pub channel_weights: Vec<(String, f32)>,
    /// Drop the validated detection if final confidence falls under this.
    pub min_confidence: f32,
}

impl ValidatorConfig {
    pub fn defaults() -> Self {
        Self {
            structural_weight: 0.5,
            channel_weights: vec![
                ("regime_alignment".into(), 1.0),
                ("multi_timeframe".into(), 1.0),
                ("historical_hit_rate".into(), 1.0),
            ],
            min_confidence: 0.55,
        }
    }

    pub fn validate(&self) -> ValidatorResult<()> {
        if !(0.0..=1.0).contains(&self.structural_weight) {
            return Err(ValidatorError::InvalidConfig(
                "structural_weight must be in 0..=1".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.min_confidence) {
            return Err(ValidatorError::InvalidConfig(
                "min_confidence must be in 0..=1".into(),
            ));
        }
        for (name, w) in &self.channel_weights {
            if *w < 0.0 {
                return Err(ValidatorError::InvalidConfig(format!(
                    "channel '{name}' weight must be >= 0"
                )));
            }
        }
        Ok(())
    }

    pub fn weight_for(&self, channel: &str) -> f32 {
        self.channel_weights
            .iter()
            .find(|(n, _)| n == channel)
            .map(|(_, w)| *w)
            .unwrap_or(1.0)
    }
}
