//! Multi-timeframe Elliott Wave confirmation — higher TF validation.
//!
//! Best Elliott Wave trades occur when patterns align across timeframes:
//!   - Signal TF: detection (e.g., M15)
//!   - Confirm TF: same pattern direction on higher TF (e.g., H1)
//!   - Bias TF: trend direction on even higher TF (e.g., D1)

use crate::corrective::CorrectiveWave;
use crate::motive::MotiveWave;

/// Multi-timeframe confirmation result.
#[derive(Debug, Clone)]
pub struct MTFConfirmation {
    /// Signal TF pattern is valid.
    pub signal_valid: bool,
    /// Confirmation TF has same-direction pattern.
    pub confirm_aligned: bool,
    /// Bias TF trend matches signal direction.
    pub bias_aligned: bool,
    /// Overall confidence (0.0 - 1.0): how many TF's align.
    pub confidence: f64,
    /// Reasoning.
    pub reason: String,
}

/// Check if motive wave has confirmation on higher TF.
///
/// Returns confidence based on:
///   - Signal TF motive valid (0.5 base)
///   - Confirmation TF motive same direction (+0.25)
///   - Bias TF trend same direction (+0.25)
pub fn check_motive_mtf(
    signal_motive: &MotiveWave,
    confirm_motive: Option<&MotiveWave>,
    bias_trend: i8, // 1=bullish, -1=bearish, 0=neutral
) -> MTFConfirmation {
    let signal_valid = true; // Assume detector already validated
    let mut confidence = 0.5;
    let mut reasons = vec!["signal_valid"];

    // Check confirmation TF alignment.
    let confirm_aligned = match confirm_motive {
        Some(cm) => cm.direction == signal_motive.direction,
        None => false,
    };

    if confirm_aligned {
        confidence += 0.25;
        reasons.push("confirm_aligned");
    } else {
        reasons.push("confirm_missing_or_misaligned");
    }

    // Check bias TF trend alignment.
    let bias_aligned = if bias_trend == 0 {
        false
    } else {
        bias_trend == signal_motive.direction
    };

    if bias_aligned {
        confidence += 0.25;
        reasons.push("bias_aligned");
    } else {
        reasons.push("bias_neutral_or_opposed");
    }

    MTFConfirmation {
        signal_valid,
        confirm_aligned,
        bias_aligned,
        confidence,
        reason: reasons.join("; "),
    }
}

/// Check if corrective wave has confirmation on higher TF.
pub fn check_corrective_mtf(
    signal_corr: &CorrectiveWave,
    confirm_corr: Option<&CorrectiveWave>,
    bias_trend: i8,
) -> MTFConfirmation {
    let signal_valid = true;
    let mut confidence = 0.5;
    let mut reasons = vec!["signal_valid"];

    // Corrective waves typically retrace larger trends, so confirmation
    // is less strict — even a flat on confirm TF is acceptable.
    let confirm_aligned = match confirm_corr {
        Some(cc) => cc.direction == signal_corr.direction || cc.direction == -signal_corr.direction,
        None => false,
    };

    if confirm_aligned {
        confidence += 0.25;
        reasons.push("confirm_aligned_or_complementary");
    } else {
        reasons.push("confirm_missing");
    }

    // Bias trend should be opposite to correction (correction retraces trend).
    let bias_aligned = if bias_trend == 0 {
        false
    } else {
        bias_trend == -signal_corr.direction
    };

    if bias_aligned {
        confidence += 0.25;
        reasons.push("bias_trending_opposite");
    } else {
        reasons.push("bias_aligned_or_neutral");
    }

    MTFConfirmation {
        signal_valid,
        confirm_aligned,
        bias_aligned,
        confidence,
        reason: reasons.join("; "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zigzag::ZigZagPoint;

    fn mock_point(bars_ago: usize, price: f64, direction: i8) -> ZigZagPoint {
        ZigZagPoint {
            bars_ago,
            price,
            direction,
        }
    }

    #[test]
    fn test_motive_mtf_full_alignment() {
        let signal = MotiveWave {
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

        let confirm = MotiveWave {
            points: [
                mock_point(4, 1000.0, 1),
                mock_point(3, 950.0, -1),
                mock_point(2, 1100.0, 1),
                mock_point(1, 1050.0, -1),
                mock_point(0, 1150.0, 1),
            ],
            direction: 1,
            score: 0.65,
        };

        let result = check_motive_mtf(&signal, Some(&confirm), 1);
        assert!(result.signal_valid);
        assert!(result.confirm_aligned);
        assert!(result.bias_aligned);
        assert_eq!(result.confidence, 1.0);
    }

    #[test]
    fn test_motive_mtf_signal_only() {
        let signal = MotiveWave {
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

        let result = check_motive_mtf(&signal, None, 0);
        assert!(result.signal_valid);
        assert!(!result.confirm_aligned);
        assert!(!result.bias_aligned);
        assert_eq!(result.confidence, 0.5);
    }

    #[test]
    fn test_corrective_mtf_bias_opposite() {
        let signal = CorrectiveWave {
            points: [
                mock_point(2, 100.0, 1),
                mock_point(1, 95.0, -1),
                mock_point(0, 85.0, 1),
            ],
            direction: -1,
            score: 0.65,
        };

        // Bias trend is opposite (upward, 1) to correction (downward, -1) ✓
        let result = check_corrective_mtf(&signal, None, 1);
        assert!(result.signal_valid);
        assert!(!result.confirm_aligned);
        assert!(result.bias_aligned);
        assert_eq!(result.confidence, 0.75);
    }
}
