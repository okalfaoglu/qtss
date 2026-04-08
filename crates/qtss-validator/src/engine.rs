//! Validator engine.
//!
//! Holds a set of [`ConfirmationChannel`] trait objects and a
//! [`ValidatorConfig`]. For each candidate detection it asks every
//! channel for an opinion, blends them with the detector's structural
//! score using config-driven weights, and emits a `ValidatedDetection`
//! when the result clears `min_confidence`.

use crate::channels::ConfirmationChannel;
use crate::config::ValidatorConfig;
use crate::context::ValidationContext;
use crate::error::ValidatorResult;
use chrono::Utc;
use qtss_domain::v2::detection::{ChannelScore, Detection, ValidatedDetection};
use std::sync::Arc;

pub struct Validator {
    config: ValidatorConfig,
    channels: Vec<Arc<dyn ConfirmationChannel>>,
}

impl Validator {
    pub fn new(config: ValidatorConfig) -> ValidatorResult<Self> {
        config.validate()?;
        Ok(Self {
            config,
            channels: Vec::new(),
        })
    }

    pub fn config(&self) -> &ValidatorConfig {
        &self.config
    }

    /// Register a confirmation channel. Order of registration is
    /// preserved in the resulting `channel_scores` slice but does not
    /// affect the blended confidence (the combiner is order-independent).
    pub fn register(&mut self, channel: Arc<dyn ConfirmationChannel>) {
        self.channels.push(channel);
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Validate a detection. Returns `Some(ValidatedDetection)` when the
    /// final blended confidence reaches `config.min_confidence`,
    /// otherwise `None`.
    pub fn validate(
        &self,
        detection: Detection,
        ctx: &ValidationContext,
    ) -> Option<ValidatedDetection> {
        let mut channel_scores: Vec<ChannelScore> = Vec::new();
        for ch in &self.channels {
            if let Some(score) = ch.evaluate(&detection, ctx) {
                let weight = self.config.weight_for(ch.name());
                channel_scores.push(ChannelScore {
                    channel: ch.name().to_string(),
                    score: score as f32,
                    weight,
                });
            }
        }
        let confidence = blend(&self.config, detection.structural_score, &channel_scores);
        if confidence < self.config.min_confidence {
            return None;
        }
        Some(ValidatedDetection {
            detection,
            channel_scores,
            confidence,
            validated_at: Utc::now(),
        })
    }
}

fn blend(
    cfg: &ValidatorConfig,
    structural: f32,
    channels: &[ChannelScore],
) -> f32 {
    // Weighted mean of structural + every voting channel.
    let mut weighted_sum = (structural as f64) * (cfg.structural_weight as f64);
    let mut weight_total = cfg.structural_weight as f64;
    let channel_budget = (1.0 - cfg.structural_weight as f64).max(0.0);

    let raw_sum: f64 = channels.iter().map(|c| c.weight as f64).sum();
    if raw_sum > 0.0 && channel_budget > 0.0 {
        for c in channels {
            // Each channel's effective weight is proportional to its
            // configured weight, scaled to fit inside the channel budget.
            let eff = (c.weight as f64 / raw_sum) * channel_budget;
            weighted_sum += (c.score as f64) * eff;
            weight_total += eff;
        }
    }
    if weight_total <= 0.0 {
        return 0.0;
    }
    (weighted_sum / weight_total).clamp(0.0, 1.0) as f32
}
