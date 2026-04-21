//! Elliott Wave corrective (ABC) pattern detector.
//!
//! A corrective wave is a 3-wave structure (A-B-C) described by 4 pivots:
//! p0 (start of A), pA (end of A), pB (end of B), pC (end of C). LuxAlgo
//! validation rules:
//!   1. Strict alternation: (p0, pA, pB, pC) must alternate High/Low.
//!   2. B retraces no more than 0.95 of A.
//!   3. C extends beyond A's endpoint in A's direction.
//!   4. C extension is in the 0.618–2.618 × A band.

use crate::luxalgo_zigzag::ZigZagPoint;

/// A detected 3-wave corrective structure described by 4 pivots.
#[derive(Debug, Clone)]
pub struct CorrectiveWave {
    /// The 4 pivots [p0, pA, pB, pC] oldest to newest.
    pub points: [ZigZagPoint; 4],
    /// Direction of correction (opposite of wave A): 1 = upward correction,
    /// -1 = downward correction.
    pub direction: i8,
    /// Structural score (0.0 - 1.0).
    pub score: f64,
}

/// Detects an ABC corrective pattern from 4 consecutive zigzag pivots.
pub fn detect_corrective(points: &[ZigZagPoint]) -> Option<CorrectiveWave> {
    if points.len() < 4 {
        return None;
    }

    let p = &points[0..4];
    let prices: [f64; 4] = [p[0].price, p[1].price, p[2].price, p[3].price];

    let a_leg = prices[1] - prices[0];
    let b_leg = prices[2] - prices[1];
    let c_leg = prices[3] - prices[2];

    if a_leg == 0.0 || b_leg == 0.0 || c_leg == 0.0 {
        return None;
    }

    // A and C share sign; B opposite.
    if a_leg.signum() != c_leg.signum() || b_leg.signum() == a_leg.signum() {
        return None;
    }

    let a_abs = a_leg.abs();
    let b_abs = b_leg.abs();
    let b_retrace = b_abs / a_abs;
    if b_retrace > 0.95 {
        return None;
    }

    // C must extend beyond end of A in A's direction.
    let c_beyond_a = if a_leg > 0.0 {
        prices[3] > prices[1]
    } else {
        prices[3] < prices[1]
    };
    if !c_beyond_a {
        return None;
    }

    let c_ext = (prices[3] - prices[1]).abs() / a_abs;
    if c_ext < 0.618 || c_ext > 2.618 {
        return None;
    }

    let score_b = score_fibonacci(b_retrace, &[0.382, 0.5, 0.618, 0.786]);
    let score_c = score_fibonacci(c_ext, &[1.0, 1.272, 1.618]);
    let score = (score_b + score_c) / 2.0;

    let a_dir = a_leg.signum() as i8;
    Some(CorrectiveWave {
        points: [p[0].clone(), p[1].clone(), p[2].clone(), p[3].clone()],
        direction: -a_dir,
        score,
    })
}

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
        ZigZagPoint { bars_ago, price, direction }
    }

    #[test]
    fn test_corrective_downward_abc() {
        // p0=110 (peak), pA=90 (A down), pB=100 (B up 50%), pC=80 (C down past pA).
        let points = vec![
            mock_point(3, 110.0, 1),
            mock_point(2, 90.0, -1),
            mock_point(1, 100.0, 1),
            mock_point(0, 80.0, -1),
        ];
        let corr = detect_corrective(&points).expect("should detect");
        assert_eq!(corr.direction, 1);
        assert!(corr.score > 0.0);
    }

    #[test]
    fn test_corrective_upward_abc() {
        let points = vec![
            mock_point(3, 100.0, -1),
            mock_point(2, 120.0, 1),
            mock_point(1, 110.0, -1),
            mock_point(0, 130.0, 1),
        ];
        let corr = detect_corrective(&points).expect("should detect");
        assert_eq!(corr.direction, -1);
    }

    #[test]
    fn test_corrective_c_not_beyond_a() {
        let points = vec![
            mock_point(3, 110.0, 1),
            mock_point(2, 90.0, -1),
            mock_point(1, 100.0, 1),
            mock_point(0, 95.0, -1), // C above pA=90 → invalid
        ];
        assert!(detect_corrective(&points).is_none());
    }

    #[test]
    fn test_insufficient_points() {
        let points = vec![
            mock_point(2, 100.0, 1),
            mock_point(1, 95.0, -1),
            mock_point(0, 85.0, 1),
        ];
        assert!(detect_corrective(&points).is_none());
    }
}
