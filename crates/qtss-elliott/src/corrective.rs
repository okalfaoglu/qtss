//! Elliott Wave corrective (ABC) pattern detector.
//!
//! A corrective wave is a 3-wave structure (A-B-C) that moves against the larger trend.
//! LuxAlgo validation rules:
//!   1. B retraces no more than 0.786 of A (otherwise likely a flat)
//!   2. C extends beyond A's endpoint in A's direction

use crate::zigzag::ZigZagPoint;

/// A detected 3-wave corrective structure.
#[derive(Debug, Clone)]
pub struct CorrectiveWave {
    /// The 3 pivots (oldest to newest).
    pub points: [ZigZagPoint; 3],
    /// Direction of correction: 1 = downward (after bullish), -1 = upward (after bearish).
    pub direction: i8,
    /// Structural score (0.0 - 1.0).
    pub score: f64,
}

/// Detects a 3-wave corrective pattern from zigzag points.
///
/// Validation (bullish A, correcting downward):
///   - A: high to low (downward move)
///   - B: low to high (retrace)
///   - C: high to low (beyond A's low)
///   - B retrace <= 0.786 of A
pub fn detect_corrective(points: &[ZigZagPoint]) -> Option<CorrectiveWave> {
    if points.len() < 3 {
        return None;
    }

    let p = &points[0..3];
    let prices: Vec<f64> = p.iter().map(|pt| pt.price).collect();

    let pa = prices[0]; // Start of A
    let pb = prices[1]; // Peak of B (end of A, start of B)
    let pc = prices[2]; // Peak of C (end of B, start of C)

    // Wave magnitudes.
    let a_move = (pb - pa).abs();
    let b_move = (pc - pb).abs();
    let c_move = (pc - pa).abs(); // C's displacement from A's start

    if a_move == 0.0 || b_move == 0.0 {
        return None;
    }

    // Determine direction (A's direction).
    let a_direction = (pb - pa).signum() as i8;
    if a_direction == 0 {
        return None;
    }

    // B retrace ratio: how much of A does B retrace?
    let b_retrace = b_move / a_move;

    // Rule 1: B should retrace <= 0.786 of A (beyond that is flat).
    if b_retrace > 0.786 {
        return None;
    }

    // Rule 2: C extends beyond A (in A's direction).
    let c_beyond_a = if a_direction > 0 {
        // A was upward, C should end below A's start.
        pc < pa
    } else {
        // A was downward, C should end above A's start.
        pc > pa
    };
    if !c_beyond_a {
        return None;
    }

    // Rule 3: C should extend 0.618 - 2.618 of A.
    let c_extension = (pc - pa).abs() / a_move;
    if c_extension < 0.618 || c_extension > 2.618 {
        return None;
    }

    // Scoring: proximity to Fibonacci multiples.
    // B retrace: 0.382, 0.5, 0.618
    // C extension: 1.0, 1.272, 1.618
    let score_b = score_fibonacci(b_retrace, &[0.382, 0.5, 0.618]);
    let score_c = score_fibonacci(c_extension, &[1.0, 1.272, 1.618]);
    let score = (score_b + score_c) / 2.0;

    Some(CorrectiveWave {
        points: [p[0].clone(), p[1].clone(), p[2].clone()],
        direction: -a_direction, // Opposite direction (correcting)
        score,
    })
}

/// Score proximity to nearest Fibonacci level.
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

    fn mock_point(bars_ago: usize, price: f64, direction: i8) -> ZigZagPoint {
        ZigZagPoint {
            bars_ago,
            price,
            direction,
        }
    }

    #[test]
    fn test_corrective_downward_abc() {
        // A: down from 100 to 90, B: up to 95, C: down to 85
        let points = vec![
            mock_point(2, 100.0, 1),  // A start
            mock_point(1, 95.0, -1),  // B peak
            mock_point(0, 85.0, 1),   // C trough
        ];

        let corr = detect_corrective(&points);
        assert!(corr.is_some());
        let c = corr.unwrap();
        assert_eq!(c.direction, -1); // Downward correction
        assert!(c.score > 0.0);
    }

    #[test]
    fn test_corrective_upward_abc() {
        // A: up from 100 to 110, B: down to 105, C: up to 115
        let points = vec![
            mock_point(2, 100.0, -1), // A start
            mock_point(1, 105.0, 1),  // B peak
            mock_point(0, 115.0, -1), // C trough
        ];

        let corr = detect_corrective(&points);
        assert!(corr.is_some());
        let c = corr.unwrap();
        assert_eq!(c.direction, 1); // Upward correction
    }

    #[test]
    fn test_corrective_b_retrace_too_high() {
        // B retraces 85% of A — invalid (>0.786).
        let points = vec![
            mock_point(2, 100.0, 1),
            mock_point(1, 91.5, -1), // 85% retrace
            mock_point(0, 80.0, 1),
        ];

        let corr = detect_corrective(&points);
        assert!(corr.is_none());
    }

    #[test]
    fn test_corrective_c_not_beyond_a() {
        // C doesn't extend beyond A's start — invalid.
        let points = vec![
            mock_point(2, 100.0, 1),
            mock_point(1, 93.0, -1),
            mock_point(0, 95.0, 1), // Doesn't go below 100
        ];

        let corr = detect_corrective(&points);
        assert!(corr.is_none());
    }

    #[test]
    fn test_insufficient_points() {
        let points = vec![mock_point(1, 100.0, 1), mock_point(0, 95.0, -1)];

        let corr = detect_corrective(&points);
        assert!(corr.is_none());
    }
}
