//! Elliott Wave invalidation rules — when patterns break.
//!
//! LuxAlgo invalidation logic:
//!   - Motive wave 2: cannot retrace beyond wave 1's start
//!   - Motive wave 4: cannot overlap wave 1's territory
//!   - Corrective: B retrace cannot exceed 0.786 (becomes flat)

use crate::corrective::CorrectiveWave;
use crate::motive::MotiveWave;
#[allow(unused_imports)]
use crate::luxalgo_zigzag::ZigZagPoint;

/// Checks if a motive wave is still valid after new price data.
/// Returns `Some(reason)` if invalid, None if still valid.
///
/// Invalidation rules (bullish frame):
///   1. Wave 2 retraces beyond wave 1's start → invalid
///   2. Wave 4 overlaps wave 1's price range → invalid
pub fn check_motive_invalid(
    motive: &MotiveWave,
    new_price: f64,
) -> Option<&'static str> {
    let p1 = motive.points[0].price;
    let p2 = motive.points[1].price;
    let p3 = motive.points[2].price;
    let p4 = motive.points[3].price;
    let p5 = motive.points[4].price;

    match motive.direction {
        1 => {
            // Bullish: 1 is low, 2 is high, 3 is low, 4 is high, 5 is low
            // Wave 2 must not retrace below wave 1's low.
            if new_price < p1 {
                return Some("wave_2_violated: retraced below wave_1_low");
            }
            // Wave 4 must not overlap wave 1 (no price overlap).
            let w1_max = p1.max(p2);
            let w1_min = p1.min(p2);
            if new_price < w1_max && new_price > w1_min {
                return Some("wave_4_overlap: entered wave_1_territory");
            }
            None
        }
        -1 => {
            // Bearish: 1 is high, 2 is low, 3 is high, 4 is low, 5 is high
            // Wave 2 must not retrace above wave 1's high.
            if new_price > p1 {
                return Some("wave_2_violated: retraced above wave_1_high");
            }
            // Wave 4 must not overlap wave 1.
            let w1_max = p1.max(p2);
            let w1_min = p1.min(p2);
            if new_price > w1_min && new_price < w1_max {
                return Some("wave_4_overlap: entered wave_1_territory");
            }
            None
        }
        _ => Some("invalid_direction"),
    }
}

/// Checks if a corrective wave is still valid after new price data.
/// Returns `Some(reason)` if invalid, None if still valid.
///
/// Invalidation rules (ABC downward correction after bullish):
///   1. B retrace exceeds 0.786 of A → becomes flat/expanded flat
pub fn check_corrective_invalid(
    corr: &CorrectiveWave,
    new_price: f64,
) -> Option<&'static str> {
    let pa = corr.points[0].price;
    let pb = corr.points[1].price;
    let pc = corr.points[2].price;

    let a_move = (pb - pa).abs();
    let b_move = (new_price - pb).abs();

    if a_move == 0.0 {
        return Some("invalid_a_wave: zero move");
    }

    let b_retrace = b_move / a_move;
    if b_retrace > 0.786 {
        return Some("b_retrace_exceeded: >0.786 (becomes flat)");
    }

    None
}

/// Predict next invalidation level for motive wave (protective stop).
pub fn motive_invalidation_level(motive: &MotiveWave) -> f64 {
    let p1 = motive.points[0].price;
    match motive.direction {
        1 => p1,  // Bullish: stop below wave 1's low
        -1 => p1, // Bearish: stop above wave 1's high
        _ => 0.0,
    }
}

/// Predict next invalidation level for corrective wave (protective stop).
pub fn corrective_invalidation_level(corr: &CorrectiveWave) -> f64 {
    let pa = corr.points[0].price;
    let pb = corr.points[1].price;
    let max_retrace_price = pa + ((pb - pa) * 0.786);
    max_retrace_price
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
    fn test_motive_bullish_valid() {
        let motive = MotiveWave {
            points: [
                mock_point(4, 100.0, 1),   // p1
                mock_point(3, 95.0, -1),   // p2
                mock_point(2, 110.0, 1),   // p3
                mock_point(1, 105.0, -1),  // p4
                mock_point(0, 115.0, 1),   // p5
            ],
            direction: 1,
            score: 0.7,
        };

        assert!(check_motive_invalid(&motive, 114.0).is_none());
    }

    #[test]
    fn test_motive_bullish_wave2_violated() {
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

        // New price below wave 1's start → invalid
        assert!(check_motive_invalid(&motive, 99.0).is_some());
    }

    #[test]
    fn test_corrective_valid() {
        let corr = CorrectiveWave {
            points: [
                mock_point(2, 100.0, 1),
                mock_point(1, 95.0, -1),
                mock_point(0, 85.0, 1),
            ],
            direction: -1,
            score: 0.7,
        };

        // B at 95% — still valid (< 0.786)
        assert!(check_corrective_invalid(&corr, 92.0).is_none());
    }

    #[test]
    fn test_corrective_b_exceeded() {
        let corr = CorrectiveWave {
            points: [
                mock_point(2, 100.0, 1),
                mock_point(1, 95.0, -1),
                mock_point(0, 85.0, 1),
            ],
            direction: -1,
            score: 0.7,
        };

        // B retraces 80% — exceeds 0.786
        assert!(check_corrective_invalid(&corr, 84.0).is_some());
    }

    #[test]
    fn test_invalidation_levels() {
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

        assert_eq!(motive_invalidation_level(&motive), 100.0);

        let corr = CorrectiveWave {
            points: [
                mock_point(2, 100.0, 1),
                mock_point(1, 95.0, -1),
                mock_point(0, 85.0, 1),
            ],
            direction: -1,
            score: 0.7,
        };

        let inv_level = corrective_invalidation_level(&corr);
        assert!(inv_level > 95.0 && inv_level < 100.0);
    }
}
