//! Pure scoring fn — see `lib.rs` doc for the contract.
//!
//! Algorithm (no scattered if/else — single fold over a typed
//! contribution table, CLAUDE.md #1):
//!
//! 1. Build a flat `Vec<LayerContribution>` from the inputs. Every
//!    layer carries `(weight, signed_score in [-1,+1])`.
//! 2. `direction` = sign of the weighted sum (or `Neutral` near zero).
//! 3. `guven` = `Σ(weight * |score| * agreement)` / `Σ(weight)` where
//!    `agreement = 1.0` if the layer matches the majority direction,
//!    `0.5` if neutral, `0.0` if it actively disagrees. This punishes
//!    contradictions instead of letting them average to zero silently.
//! 4. `layer_count = contributions.len()`. If `< min_layers` →
//!    `guven = 0` (direction is preserved).
//! 5. `erken_uyari = tbm_score.unwrap_or(0)` — pass-through.

use crate::types::{
    ConfluenceDirection, ConfluenceInputs, ConfluenceReading, ConfluenceWeights, DetectionVote,
};

struct LayerContribution {
    label: String,
    weight: f64,
    /// Signed score in `[-1, +1]`. + = long, - = short.
    signed: f64,
}

fn detection_signed(v: &DetectionVote) -> f64 {
    v.direction.sign() * (v.structural_score as f64).clamp(0.0, 1.0)
}

fn family_weight(weights: &ConfluenceWeights, family: &str) -> f64 {
    match family {
        "elliott" => weights.elliott,
        "harmonic" => weights.harmonic,
        "classical" => weights.classical,
        "wyckoff" => weights.wyckoff,
        "range" => weights.range,
        _ => 0.0, // unknown family ignored — config-driven additions go through here
    }
}

fn build_contributions(
    inputs: &ConfluenceInputs,
    weights: &ConfluenceWeights,
) -> Vec<LayerContribution> {
    let mut out: Vec<LayerContribution> = Vec::new();

    if let Some(score) = inputs.tbm_score {
        // TBM confidence acts as a soft weight multiplier so a low-
        // confidence TBM read doesn't drown out structural detectors.
        let confidence = inputs.tbm_confidence.unwrap_or(1.0).clamp(0.0, 1.0);
        out.push(LayerContribution {
            label: format!("[TBM] score {score:+.2} (conf {confidence:.2})"),
            weight: weights.tbm * confidence,
            signed: score.clamp(-1.0, 1.0),
        });
    }

    if let Some(score) = inputs.onchain {
        out.push(LayerContribution {
            label: format!("[ON] aggregate {score:+.2}"),
            weight: weights.onchain,
            signed: score.clamp(-1.0, 1.0),
        });
    }

    for v in &inputs.detections {
        let w = family_weight(weights, v.family.as_str());
        if w <= 0.0 {
            continue;
        }
        let signed = detection_signed(v);
        out.push(LayerContribution {
            label: format!(
                "[{}] {} {:+.2}",
                v.family.to_ascii_uppercase(),
                v.subkind,
                signed
            ),
            weight: w,
            signed,
        });
    }

    out
}

fn majority_direction(contribs: &[LayerContribution]) -> ConfluenceDirection {
    let weighted: f64 = contribs.iter().map(|c| c.weight * c.signed).sum();
    if weighted > 0.05 {
        ConfluenceDirection::Long
    } else if weighted < -0.05 {
        ConfluenceDirection::Short
    } else {
        ConfluenceDirection::Neutral
    }
}

/// Agreement multiplier for one layer relative to the majority.
/// Disagreement zeroes the layer's contribution to `guven` instead of
/// letting + and - cancel silently.
fn agreement(layer_signed: f64, majority: ConfluenceDirection) -> f64 {
    let layer_dir = if layer_signed > 0.05 {
        ConfluenceDirection::Long
    } else if layer_signed < -0.05 {
        ConfluenceDirection::Short
    } else {
        ConfluenceDirection::Neutral
    };
    match (layer_dir, majority) {
        (a, b) if a == b => 1.0,
        (ConfluenceDirection::Neutral, _) | (_, ConfluenceDirection::Neutral) => 0.5,
        _ => 0.0, // active disagreement
    }
}

pub fn score_confluence(
    inputs: &ConfluenceInputs,
    weights: &ConfluenceWeights,
) -> ConfluenceReading {
    let contribs = build_contributions(inputs, weights);
    let layer_count = contribs.len() as u32;

    let erken_uyari = inputs.tbm_score.unwrap_or(0.0).clamp(-1.0, 1.0);

    if contribs.is_empty() {
        return ConfluenceReading {
            erken_uyari,
            guven: 0.0,
            direction: ConfluenceDirection::Neutral,
            layer_count: 0,
            details: vec!["no fresh inputs".to_string()],
        };
    }

    let direction = majority_direction(&contribs);

    let mut weighted_strength = 0.0_f64;
    let mut wsum = 0.0_f64;
    let mut details: Vec<String> = Vec::with_capacity(contribs.len() + 1);
    for c in &contribs {
        let agr = agreement(c.signed, direction);
        weighted_strength += c.weight * c.signed.abs() * agr;
        wsum += c.weight;
        details.push(c.label.clone());
    }

    let raw_guven = if wsum < 1e-9 {
        0.0
    } else {
        (weighted_strength / wsum).clamp(0.0, 1.0)
    };

    let guven = if layer_count < weights.min_layers {
        details.push(format!(
            "guven=0 (layers={layer_count} < min_layers={})",
            weights.min_layers
        ));
        0.0
    } else {
        raw_guven
    };

    ConfluenceReading {
        erken_uyari,
        guven,
        direction,
        layer_count,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vote(family: &str, dir: ConfluenceDirection, score: f32) -> DetectionVote {
        DetectionVote {
            family: family.to_string(),
            subkind: "x".to_string(),
            direction: dir,
            structural_score: score,
        }
    }

    #[test]
    fn empty_inputs_neutral() {
        let r = score_confluence(&ConfluenceInputs::default(), &ConfluenceWeights::default());
        assert_eq!(r.direction, ConfluenceDirection::Neutral);
        assert_eq!(r.guven, 0.0);
        assert_eq!(r.layer_count, 0);
    }

    #[test]
    fn single_layer_below_min_zeros_guven() {
        let inputs = ConfluenceInputs {
            tbm_score: Some(0.8),
            tbm_confidence: Some(1.0),
            ..Default::default()
        };
        let r = score_confluence(&inputs, &ConfluenceWeights::default());
        assert_eq!(r.layer_count, 1);
        assert_eq!(r.direction, ConfluenceDirection::Long); // direction preserved
        assert_eq!(r.guven, 0.0); // hard zero — only 1 layer, min_layers=3
    }

    #[test]
    fn three_aligned_layers_unlocks_guven() {
        let inputs = ConfluenceInputs {
            tbm_score: Some(0.8),
            tbm_confidence: Some(1.0),
            onchain: Some(0.6),
            detections: vec![vote("elliott", ConfluenceDirection::Long, 0.9)],
        };
        let r = score_confluence(&inputs, &ConfluenceWeights::default());
        assert_eq!(r.layer_count, 3);
        assert_eq!(r.direction, ConfluenceDirection::Long);
        assert!(r.guven > 0.5, "expected confident long, got {}", r.guven);
    }

    #[test]
    fn contradictions_reduce_guven() {
        let inputs = ConfluenceInputs {
            tbm_score: Some(0.8),
            tbm_confidence: Some(1.0),
            onchain: Some(-0.7),
            detections: vec![
                vote("elliott", ConfluenceDirection::Long, 0.9),
                vote("harmonic", ConfluenceDirection::Short, 0.8),
            ],
        };
        let r = score_confluence(&inputs, &ConfluenceWeights::default());
        assert_eq!(r.layer_count, 4);
        // Net direction will be one side, but guven must be capped low
        // because half the layers are voting against it.
        assert!(r.guven < 0.6, "expected diluted guven, got {}", r.guven);
    }

    #[test]
    fn erken_uyari_passthrough_independent_of_min_layers() {
        let inputs = ConfluenceInputs {
            tbm_score: Some(-0.92),
            tbm_confidence: Some(0.8),
            ..Default::default()
        };
        let r = score_confluence(&inputs, &ConfluenceWeights::default());
        assert!((r.erken_uyari - -0.92).abs() < 1e-9);
        assert_eq!(r.guven, 0.0); // gated by min_layers
        assert_eq!(r.direction, ConfluenceDirection::Short); // direction kept
    }
}
