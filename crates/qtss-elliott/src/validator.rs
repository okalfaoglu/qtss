//! Elliott Wave pattern validator — rules and score refinement.
//!
//! Validates detected motive/corrective patterns against stricter criteria:
//!   - Structural score threshold
//!   - Risk/reward ratio
//!   - Invalidation distance (cushion)

use crate::corrective::CorrectiveWave;
use crate::invalidation::{check_corrective_invalid, check_motive_invalid};
use crate::motive::MotiveWave;
use crate::targets::{corrective_primary_target, motive_primary_target, project_corrective_targets, project_motive_targets};
#[cfg(test)]
use crate::luxalgo_zigzag::ZigZagPoint;

/// Validation result for a pattern.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Pattern is valid (passes all checks).
    pub valid: bool,
    /// Confidence score (0.0 - 1.0).
    pub confidence: f64,
    /// Reason if invalid.
    pub reason: Option<String>,
    /// Distance to invalidation level (positive = safe).
    pub invalidation_cushion: f64,
    /// Risk/reward ratio (target / stop).
    pub risk_reward: f64,
}

/// Validate motive wave against strict criteria.
pub fn validate_motive(
    motive: &MotiveWave,
    current_price: f64,
    min_structural_score: f32,
    min_risk_reward: f64,
) -> ValidationResult {
    // Structural score check.
    if motive.score < min_structural_score as f64 {
        return ValidationResult {
            valid: false,
            confidence: motive.score,
            reason: Some(format!(
                "structural_score_too_low: {:.3} < {:.3}",
                motive.score, min_structural_score
            )),
            invalidation_cushion: 0.0,
            risk_reward: 0.0,
        };
    }

    // Invalidation check (current price should not have broken pattern yet).
    if let Some(reason) = check_motive_invalid(motive, current_price) {
        return ValidationResult {
            valid: false,
            confidence: motive.score,
            reason: Some(reason.to_string()),
            invalidation_cushion: 0.0,
            risk_reward: 0.0,
        };
    }

    // Calculate risk/reward.
    let p1 = motive.points[0].price;
    let targets = project_motive_targets(motive);
    let target = motive_primary_target(&targets);
    let stop = p1;

    let risk = (current_price - stop).abs();
    let reward = (target - current_price).abs();
    let rr = if risk > 0.0 { reward / risk } else { 0.0 };

    if rr < min_risk_reward {
        return ValidationResult {
            valid: false,
            confidence: motive.score,
            reason: Some(format!("risk_reward_too_low: {:.2} < {:.2}", rr, min_risk_reward)),
            invalidation_cushion: risk,
            risk_reward: rr,
        };
    }

    // Invalidation cushion: how far current price is from invalidation level.
    let cushion = if motive.direction > 0 {
        current_price - stop
    } else {
        stop - current_price
    };

    ValidationResult {
        valid: true,
        confidence: motive.score,
        reason: None,
        invalidation_cushion: cushion.abs(),
        risk_reward: rr,
    }
}

/// Validate corrective wave against strict criteria.
pub fn validate_corrective(
    corr: &CorrectiveWave,
    current_price: f64,
    min_structural_score: f32,
    min_risk_reward: f64,
) -> ValidationResult {
    // Structural score check.
    if corr.score < min_structural_score as f64 {
        return ValidationResult {
            valid: false,
            confidence: corr.score,
            reason: Some(format!(
                "structural_score_too_low: {:.3} < {:.3}",
                corr.score, min_structural_score
            )),
            invalidation_cushion: 0.0,
            risk_reward: 0.0,
        };
    }

    // Invalidation check.
    if let Some(reason) = check_corrective_invalid(corr, current_price) {
        return ValidationResult {
            valid: false,
            confidence: corr.score,
            reason: Some(reason.to_string()),
            invalidation_cushion: 0.0,
            risk_reward: 0.0,
        };
    }

    // Calculate risk/reward.
    let pb = corr.points[1].price;
    let targets = project_corrective_targets(corr);
    let target = corrective_primary_target(&targets);
    let stop = pb;

    let risk = (current_price - stop).abs();
    let reward = (target - current_price).abs();
    let rr = if risk > 0.0 { reward / risk } else { 0.0 };

    if rr < min_risk_reward {
        return ValidationResult {
            valid: false,
            confidence: corr.score,
            reason: Some(format!("risk_reward_too_low: {:.2} < {:.2}", rr, min_risk_reward)),
            invalidation_cushion: risk,
            risk_reward: rr,
        };
    }

    // Invalidation cushion.
    let cushion = (current_price - stop).abs();

    ValidationResult {
        valid: true,
        confidence: corr.score,
        reason: None,
        invalidation_cushion: cushion,
        risk_reward: rr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_point(bars_ago: usize, price: f64, direction: i8) -> ZigZagPoint {
        ZigZagPoint {
            bars_ago,
            price,
            direction,
        }
    }

    #[test]
    fn test_validate_motive_valid() {
        let motive = MotiveWave {
            points: [
                mock_point(4, 100.0, 1),
                mock_point(3, 95.0, -1),
                mock_point(2, 110.0, 1),
                mock_point(1, 105.0, -1),
                mock_point(0, 115.0, 1),
            ],
            direction: 1,
            score: 0.7,
        };

        let result = validate_motive(&motive, 112.0, 0.5, 1.0);
        assert!(result.valid);
        assert!(result.risk_reward > 1.0);
    }

    #[test]
    fn test_validate_motive_low_score() {
        let motive = MotiveWave {
            points: [
                mock_point(4, 100.0, 1),
                mock_point(3, 95.0, -1),
                mock_point(2, 110.0, 1),
                mock_point(1, 105.0, -1),
                mock_point(0, 115.0, 1),
            ],
            direction: 1,
            score: 0.3,
        };

        let result = validate_motive(&motive, 112.0, 0.5, 1.0);
        assert!(!result.valid);
        assert!(result.reason.is_some());
    }

    #[test]
    fn test_validate_motive_invalidated() {
        let motive = MotiveWave {
            points: [
                mock_point(4, 100.0, 1),
                mock_point(3, 95.0, -1),
                mock_point(2, 110.0, 1),
                mock_point(1, 105.0, -1),
                mock_point(0, 115.0, 1),
            ],
            direction: 1,
            score: 0.7,
        };

        // Price below wave 1's start → invalid
        let result = validate_motive(&motive, 99.0, 0.5, 1.0);
        assert!(!result.valid);
    }

    #[test]
    fn test_validate_corrective_valid() {
        let corr = CorrectiveWave {
            points: [
                mock_point(2, 100.0, 1),
                mock_point(1, 95.0, -1),
                mock_point(0, 85.0, 1),
            ],
            direction: -1,
            score: 0.65,
        };

        let result = validate_corrective(&corr, 88.0, 0.5, 1.0);
        assert!(result.valid);
    }
}
