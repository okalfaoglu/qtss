//! Elliott Wave motive (impulse) pattern detector — LuxAlgo Pine 1:1.
//!
//! Reference: `reference/luxalgo/elliott_wave_luxalgo.pine` (© LuxAlgo,
//! CC BY-NC-SA 4.0). The Pine indicator's bullish guard is:
//! ```pine
//! _W5 = _6y - _5y ; _W3 = _4y - _3y ; _W1 = _2y - _1y
//! min = math.min(_W1, _W3, _W5)
//! isWave = _W3 != min and _6y > _4y and _3y > _1y and _5y > _2y
//! ```
//! (bearish uses mirrored inequalities on the same six pivots). We port
//! every clause — dropping any of them silences real Pine-positive cases
//! elsewhere in the tape, e.g. an impulse where W3 is the shortest leg
//! or a W4 wick that dips into W1 territory.
//!
//! Rule mapping (our p0..p5 == Pine _1..._6, oldest to newest):
//!   1. Strict pivot alternation (H/L/H/L/H/L) — pre-filter.
//!   2. Wave 3 is NOT the shortest of {|W1|,|W3|,|W5|}       → Pine `_W3 != min`
//!   3. Wave 5 extends beyond Wave 3 endpoint                → Pine `_6y > _4y`
//!   4. Wave 2 does not fully retrace Wave 1 (no p0 cross)   → Pine `_3y > _1y`
//!   5. Wave 4 does not enter Wave 1 territory (no overlap)  → Pine `_5y > _2y`

use crate::luxalgo_zigzag::ZigZagPoint;

/// A detected 5-wave motive structure described by 6 pivots.
#[derive(Debug, Clone)]
pub struct MotiveWave {
    /// The 6 pivots [p0..p5] oldest to newest.
    pub points: [ZigZagPoint; 6],
    /// Direction: 1 = bullish, -1 = bearish.
    pub direction: i8,
    /// Structural score (0.0 - 1.0).
    pub score: f64,
}

/// Detects a 5-wave motive pattern from 6 consecutive zigzag pivots.
pub fn detect_motive(points: &[ZigZagPoint]) -> Option<MotiveWave> {
    if points.len() < 6 {
        return None;
    }

    let p = &points[0..6];
    let px: [f64; 6] = [
        p[0].price,
        p[1].price,
        p[2].price,
        p[3].price,
        p[4].price,
        p[5].price,
    ];

    let w1 = px[1] - px[0];
    let w2 = px[2] - px[1];
    let w3 = px[3] - px[2];
    let w4 = px[4] - px[3];
    let w5 = px[5] - px[4];

    if w1 == 0.0 || w2 == 0.0 || w3 == 0.0 || w4 == 0.0 || w5 == 0.0 {
        return None;
    }

    // W1, W3, W5 must share direction; W2, W4 must be opposite.
    let dir = w1.signum();
    if w3.signum() != dir || w5.signum() != dir {
        return None;
    }
    if w2.signum() == dir || w4.signum() == dir {
        return None;
    }
    let direction = dir as i8;

    // Pine `_3y > _1y` (bull) / `_1y > _3y` (bear): W2 doesn't fully
    // retrace W1 — p2 stays on W1's side of p0.
    let w2_not_full_retrace = if direction > 0 { px[2] > px[0] } else { px[2] < px[0] };
    if !w2_not_full_retrace {
        return None;
    }

    // Pine `_6y > _4y` (bull) / `_4y > _6y` (bear): W5 extends past W3
    // endpoint — the hallmark 5th-wave continuation.
    let w5_beyond_w3 = if direction > 0 { px[5] > px[3] } else { px[5] < px[3] };
    if !w5_beyond_w3 {
        return None;
    }

    // Pine `_5y > _2y` (bull) / `_2y > _5y` (bear): W4 must NOT enter
    // W1 territory — p4 stays on W1 endpoint's side of p1. Classic
    // Elliott no-overlap rule; dropping this conflates impulses with
    // leading-diagonal / 3-wave structures and produces the false
    // impulses the TV user flagged.
    let w4_no_w1_overlap = if direction > 0 { px[4] > px[1] } else { px[4] < px[1] };
    if !w4_no_w1_overlap {
        return None;
    }

    // Pine `_W3 != math.min(_W1, _W3, _W5)`: W3 is not the shortest of
    // {|W1|,|W3|,|W5|}. Uses signed Pine magnitudes (direction-aligned)
    // → equivalent to comparing absolute values for our same-sign legs.
    let min_leg = w1.abs().min(w3.abs()).min(w5.abs());
    if w3.abs() == min_leg {
        return None;
    }

    // Fibonacci scoring.
    let w1_abs = w1.abs();
    let w3_abs = w3.abs();
    let w2_ratio = w2.abs() / w1_abs;
    let w3_ratio = w3_abs / w1_abs;
    let w4_ratio = w4.abs() / w3_abs;

    let score_w2 = score_fibonacci(w2_ratio, &[0.382, 0.5, 0.618]);
    let score_w3 = score_fibonacci(w3_ratio, &[1.618, 2.618]);
    let score_w4 = score_fibonacci(w4_ratio, &[0.236, 0.382]);
    let score = (score_w2 + score_w3 + score_w4) / 3.0;

    Some(MotiveWave {
        points: [
            p[0].clone(),
            p[1].clone(),
            p[2].clone(),
            p[3].clone(),
            p[4].clone(),
            p[5].clone(),
        ],
        direction,
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
    fn test_motive_bullish() {
        // p0=100 (start), p1=120 (W1 up), p2=110 (W2 retrace), p3=145 (W3 up),
        // p4=130 (W4 retrace, > p1=120 so no overlap), p5=160 (W5 up beyond p3).
        let points = vec![
            mock_point(5, 100.0, -1),
            mock_point(4, 120.0, 1),
            mock_point(3, 110.0, -1),
            mock_point(2, 145.0, 1),
            mock_point(1, 130.0, -1),
            mock_point(0, 160.0, 1),
        ];
        let motive = detect_motive(&points).expect("should detect bullish motive");
        assert_eq!(motive.direction, 1);
        assert!(motive.score > 0.0);
    }

    #[test]
    fn test_motive_bearish() {
        let points = vec![
            mock_point(5, 100.0, 1),
            mock_point(4, 80.0, -1),
            mock_point(3, 90.0, 1),
            mock_point(2, 55.0, -1),
            mock_point(1, 70.0, 1),
            mock_point(0, 40.0, -1),
        ];
        let motive = detect_motive(&points).expect("should detect bearish motive");
        assert_eq!(motive.direction, -1);
    }

    #[test]
    fn test_motive_rejects_w4_overlap_with_w1() {
        // p4=115 < p1=120 — W4 dips into W1 territory. Pine enforces
        // `_5y > _2y` strictly, so this must be rejected (1:1 parity).
        let points = vec![
            mock_point(5, 100.0, -1),
            mock_point(4, 120.0, 1),
            mock_point(3, 110.0, -1),
            mock_point(2, 145.0, 1),
            mock_point(1, 115.0, -1),
            mock_point(0, 160.0, 1),
        ];
        assert!(detect_motive(&points).is_none());
    }

    #[test]
    fn test_motive_rejects_w3_shortest() {
        // |W1|=20, |W3|=15, |W5|=30 → W3 is the shortest, Pine's
        // `_W3 != math.min(_W1, _W3, _W5)` rule rejects this.
        let points = vec![
            mock_point(5, 100.0, -1),
            mock_point(4, 120.0, 1),   // W1 = +20
            mock_point(3, 110.0, -1),  // W2 = -10
            mock_point(2, 125.0, 1),   // W3 = +15 (shortest)
            mock_point(1, 121.0, -1),  // W4 = -4 (no overlap with p1=120)
            mock_point(0, 151.0, 1),   // W5 = +30
        ];
        assert!(detect_motive(&points).is_none());
    }

    #[test]
    fn test_motive_rejects_wave2_full_retrace() {
        // W2 retraces 100% of W1 (p2 <= p0) — must be rejected.
        let points = vec![
            mock_point(5, 100.0, -1),
            mock_point(4, 120.0, 1),
            mock_point(3, 95.0, -1), // below p0=100 → W2 retraced >100%
            mock_point(2, 145.0, 1),
            mock_point(1, 130.0, -1),
            mock_point(0, 160.0, 1),
        ];
        assert!(detect_motive(&points).is_none());
    }

    #[test]
    fn test_insufficient_points() {
        let points = vec![
            mock_point(4, 100.0, 1),
            mock_point(3, 95.0, -1),
            mock_point(2, 110.0, 1),
            mock_point(1, 105.0, -1),
            mock_point(0, 115.0, 1),
        ];
        assert!(detect_motive(&points).is_none());
    }
}
