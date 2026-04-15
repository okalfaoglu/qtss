//! Combine per-category readings into one aggregate score that the
//! TBM Onchain pillar consumes.
//!
//! Single dispatch point (CLAUDE.md #1): each [`CategoryKind`] maps to
//! one weight, and we never branch on category anywhere else.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::types::{CategoryKind, CategoryReading, OnchainDirection};

/// Per-category weights, summed to (roughly) 1.0. Sourced from
/// `system_config` (`onchain.aggregator.weights`).
#[derive(Debug, Clone, Copy)]
pub struct AggregatorWeights {
    pub derivatives: f64,
    pub stablecoin: f64,
    pub chain: f64,
}

impl AggregatorWeights {
    fn weight_for(&self, k: CategoryKind) -> f64 {
        match k {
            CategoryKind::Derivatives => self.derivatives,
            CategoryKind::Stablecoin => self.stablecoin,
            CategoryKind::Chain => self.chain,
        }
    }
}

impl Default for AggregatorWeights {
    fn default() -> Self {
        // Derivatives is the only signal that exists for every symbol,
        // so it carries the most weight. Chain is BTC/ETH-only and
        // optional, hence the smallest share.
        Self { derivatives: 0.5, stablecoin: 0.3, chain: 0.2 }
    }
}

/// Output consumed by the worker → TBM provider bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateOnchain {
    /// Final score in `[0, 1]` (re-mapped from `[-1, +1]`). 0.5 is neutral.
    pub score: f64,
    pub direction: OnchainDirection,
    pub confidence: f64,
    pub per_category: HashMap<String, f64>,
    pub details: Vec<String>,
}

/// Blend `readings` using `weights`. Empty input → neutral score with
/// zero confidence so the TBM pillar correctly mutes itself.
///
/// Backwards-compatible wrapper for callers that do not yet pass a
/// timeframe. Includes every reading regardless of cadence.
#[must_use]
pub fn aggregate(readings: &[CategoryReading], weights: AggregatorWeights) -> AggregateOnchain {
    aggregate_for_tf(readings, weights, u64::MAX)
}

/// TF-aware blend (Faz 7.7 / P29a). Skips readings whose native cadence
/// is coarser than `tf_s`: a 15-minute TBM setup (tf_s=900) must never
/// inherit yesterday's stablecoin snapshot (cadence_s=86_400), since
/// that reading does not change meaningfully on the caller's horizon
/// and would force a constant bias into the aggregate.
///
/// Pass `u64::MAX` to accept every reading (legacy behaviour).
#[must_use]
pub fn aggregate_for_tf(
    readings: &[CategoryReading],
    weights: AggregatorWeights,
    tf_s: u64,
) -> AggregateOnchain {
    let filtered: Vec<&CategoryReading> = readings
        .iter()
        .filter(|r| r.cadence_s <= tf_s)
        .collect();
    if filtered.is_empty() {
        return AggregateOnchain {
            score: 0.5,
            direction: OnchainDirection::Neutral,
            confidence: 0.0,
            per_category: HashMap::new(),
            details: vec!["no onchain readings".into()],
        };
    }

    let mut weighted_sum = 0.0_f64;
    let mut weight_sum = 0.0_f64;
    let mut conf_sum = 0.0_f64;
    let mut per_category: HashMap<String, f64> = HashMap::new();
    let mut details: Vec<String> = Vec::new();

    for r in &filtered {
        let base = weights.weight_for(r.category);
        let w = base * r.confidence;
        if w <= 0.0 {
            continue;
        }
        weighted_sum += r.score.clamp(-1.0, 1.0) * w;
        weight_sum += w;
        conf_sum += r.confidence * base;
        per_category.insert(format!("{:?}", r.category).to_lowercase(), r.score);
        details.extend(r.details.iter().cloned());
    }

    if weight_sum == 0.0 {
        return AggregateOnchain {
            score: 0.5,
            direction: OnchainDirection::Neutral,
            confidence: 0.0,
            per_category,
            details,
        };
    }

    // Mean signed score in [-1, 1] → 0..1
    let signed = weighted_sum / weight_sum;
    let score = ((signed + 1.0) * 0.5).clamp(0.0, 1.0);

    let direction = if signed > 0.05 {
        OnchainDirection::Long
    } else if signed < -0.05 {
        OnchainDirection::Short
    } else {
        OnchainDirection::Neutral
    };

    // Confidence: how much of the maximum possible weight we filled.
    let max_weight = weights.derivatives + weights.stablecoin + weights.chain;
    let confidence = if max_weight > 0.0 {
        (conf_sum / max_weight).clamp(0.0, 1.0)
    } else {
        0.0
    };

    AggregateOnchain { score, direction, confidence, per_category, details }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(cat: CategoryKind, score: f64, conf: f64) -> CategoryReading {
        CategoryReading {
            category: cat,
            score,
            confidence: conf,
            direction: None,
            details: vec![],
            cadence_s: 0,
        }
    }

    #[test]
    fn empty_is_neutral_zero_confidence() {
        let a = aggregate(&[], AggregatorWeights::default());
        assert_eq!(a.confidence, 0.0);
        assert_eq!(a.direction, OnchainDirection::Neutral);
        assert!((a.score - 0.5).abs() < 1e-9);
    }

    #[test]
    fn all_bull_pushes_above_half() {
        let a = aggregate(
            &[
                r(CategoryKind::Derivatives, 0.8, 0.9),
                r(CategoryKind::Stablecoin, 0.6, 0.7),
            ],
            AggregatorWeights::default(),
        );
        assert!(a.score > 0.5);
        assert_eq!(a.direction, OnchainDirection::Long);
    }

    #[test]
    fn all_bear_pushes_below_half() {
        let a = aggregate(
            &[
                r(CategoryKind::Derivatives, -0.7, 0.9),
                r(CategoryKind::Chain, -0.5, 0.8),
            ],
            AggregatorWeights::default(),
        );
        assert!(a.score < 0.5);
        assert_eq!(a.direction, OnchainDirection::Short);
    }
}
