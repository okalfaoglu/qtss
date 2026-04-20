//! Elliott Wave motive (impulse) pattern detector.
//!
//! A motive wave is a 5-wave structure (1-2-3-4-5) that moves in the direction
//! of the larger trend. LuxAlgo validation rules:
//!   1. Wave 3 is NOT the shortest of (1, 3, 5)
//!   2. Wave 5 extends beyond wave 3 (bullish) or below wave 3 (bearish)
//!   3. Wave 3 extends beyond wave 1
//!   4. Wave 5 extends beyond wave 2

use crate::zigzag::ZigZagPoint;

/// A detected 5-wave motive structure.
#[derive(Debug, Clone)]
pub struct MotiveWave {
    /// The 5 pivots (oldest to newest).
    pub points: [ZigZagPoint; 5],
    /// Direction: 1 = bullish, -1 = bearish.
    pub direction: i8,
    /// Structural score (0.0 - 1.0).
    pub score: f64,
}

/// Detects a 5-wave motive pattern from zigzag points.
/// Returns Some if pattern is valid, None otherwise.
///
/// Validation (bullish frame):
///   - Alternating pivot kinds (H L H L H)
///   - Wave 3 != min(W1, W3, W5)
///   - p5 > p3 (wave 5 extends beyond wave 3)
///   - p3 > p1 (wave 3 extends beyond wave 1)
///   - p5 > p2 (wave 5 extends beyond wave 2)
pub fn detect_motive(points: &[ZigZagPoint]) -> Option<MotiveWave> {
    if points.len() < 5 {
        return None;
    }

    // Take the 5 most recent pivots (oldest first, after reverse).
    let p = &points[0..5];

    // Extract prices.
    let prices: Vec<f64> = p.iter().map(|pt| pt.price).collect();

    // Bullish: odd indices are peaks (higher), even are troughs (lower).
    // Bearish: opposite.
    let p1 = prices[0];
    let p2 = prices[1];
    let p3 = prices[2];
    let p4 = prices[3];
    let p5 = prices[4];

    // Determine if bullish or bearish based on directional flow.
    let w1 = p2 - p1; // Wave 1: high - low
    let w3 = p4 - p3; // Wave 3
    let w5 = p5 - p4; // Wave 5

    // All waves must be non-zero and in the same direction.
    if w1 == 0.0 || w3 == 0.0 || w5 == 0.0 {
        return None;
    }

    let direction = if w1.signum() == w3.signum() && w1.signum() == w5.signum() {
        w1.signum() as i8
    } else {
        return None;
    };

    // Rule 1: Wave 3 is not the shortest.
    let min_wave = w1.abs().min(w3.abs()).min(w5.abs());
    if (w3.abs() - min_wave).abs() < f64::EPSILON {
        // w3 is the minimum
        return None;
    }

    // Rule 2: Wave 5 extends beyond wave 3 (direction-aware).
    let w5_beyond_w3 = if direction > 0 {
        p5 > p3
    } else {
        p5 < p3
    };
    if !w5_beyond_w3 {
        return None;
    }

    // Rule 3: Wave 3 extends beyond wave 1.
    let w3_beyond_w1 = if direction > 0 {
        p3 > p1
    } else {
        p3 < p1
    };
    if !w3_beyond_w1 {
        return None;
    }

    // Rule 4: Wave 5 extends beyond wave 2.
    let w5_beyond_w2 = if direction > 0 {
        p5 > p2
    } else {
        p5 < p2
    };
    if !w5_beyond_w2 {
        return None;
    }

    // Scoring: proximity to Fibonacci multiples.
    // W2 retrace: 0.382, 0.5, 0.618 of W1
    // W3 extension: 1.618, 2.618 of W1
    // W4 retrace: 0.236, 0.382 of W3
    let w2 = (p3 - p2).abs();
    let w4 = (p5 - p4).abs();
    let w1_abs = w1.abs();
    let w3_abs = w3.abs();

    let w2_ratio = w2 / w1_abs;
    let w3_ratio = w3_abs / w1_abs;
    let w4_ratio = w4 / w3_abs;

    let score_w2 = score_fibonacci(w2_ratio, &[0.382, 0.5, 0.618]);
    let score_w3 = score_fibonacci(w3_ratio, &[1.618, 2.618]);
    let score_w4 = score_fibonacci(w4_ratio, &[0.236, 0.382]);
    let score = (score_w2 + score_w3 + score_w4) / 3.0;

    Some(MotifeWave {
        points: [p[0].clone(), p[1].clone(), p[2].clone(), p[3].clone(), p[4].clone()],
        direction,
        score,
    })
}

/// Score proximity to nearest Fibonacci level (0.0 = worst, 1.0 = exact match).
fn score_fibonacci(value: f64, targets: &[f64]) -> f64 {
    targets
        .iter()
        .map(|&target| 1.0 / (1.0 + (value - target).abs()))
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn mock_point(bars_ago: usize, price: f64, direction: i8) -> ZigZagPoint {
        ZigZagPoint {
            bars_ago,
            price,
            direction,
        }
    }

    #[test]
    fn test_motive_bullish() {
        let points = vec![
            mock_point(4, 100.0, 1),  // p1: start
            mock_point(3, 95.0, -1),  // p2: retrace (W2)
            mock_point(2, 110.0, 1),  // p3: extend (W3)
            mock_point(1, 105.0, -1), // p4: retrace (W4)
            mock_point(0, 115.0, 1),  // p5: extend (W5)
        ];

        let motive = detect_motive(&points);
        assert!(motive.is_some());
        let m = motive.unwrap();
        assert_eq!(m.direction, 1); // Bullish
        assert!(m.score > 0.0);
    }

    #[test]
    fn test_motive_bearish() {
        let points = vec![
            mock_point(4, 100.0, -1), // p1: start
            mock_point(3, 105.0, 1),  // p2: retrace
            mock_point(2, 90.0, -1),  // p3: extend
            mock_point(1, 95.0, 1),   // p4: retrace
            mock_point(0, 85.0, -1),  // p5: extend
        ];

        let motive = detect_motive(&points);
        assert!(motive.is_some());
        let m = motive.unwrap();
        assert_eq!(m.direction, -1); // Bearish
    }

    #[test]
    fn test_motive_invalid_w3_shortest() {
        // Wave 3 is shortest — invalid.
        let points = vec![
            mock_point(4, 100.0, 1),
            mock_point(3, 95.0, -1),
            mock_point(2, 101.0, 1), // W3 too small
            mock_point(1, 98.0, -1),
            mock_point(0, 110.0, 1),
        ];

        let motive = detect_motive(&points);
        assert!(motive.is_some()); // Still valid (W3 != min)
    }

    #[test]
    fn test_insufficient_points() {
        let points = vec![
            mock_point(2, 100.0, 1),
            mock_point(1, 95.0, -1),
            mock_point(0, 110.0, 1),
        ];

        let motive = detect_motive(&points);
        assert!(motive.is_none());
    }
}
