//! Elliott Wave target projection — fibonacci-based price targets.
//!
//! For motive waves (5-wave): project wave 5 target based on W1, W3.
//! For corrective waves (ABC): project C target based on A, B.

use crate::corrective::CorrectiveWave;
use crate::motive::MotiveWave;

/// Wave 5 target projections (common ratios).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MotifeTargets {
    /// 1.0× of wave 1
    pub target_1_0: f64,
    /// 1.272× of wave 1
    pub target_1_272: f64,
    /// 1.618× of wave 1 (golden ratio)
    pub target_1_618: f64,
    /// 0.5× of wave 3
    pub target_0_5_w3: f64,
}

/// Wave C target projections.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CorrectiveTargets {
    /// 1.0× of wave A
    pub target_1_0: f64,
    /// 1.272× of wave A
    pub target_1_272: f64,
    /// 1.618× of wave A
    pub target_1_618: f64,
}

/// Project wave 5 targets from wave 1 and wave 3 magnitudes.
pub fn project_motive_targets(motive: &MotiveWave) -> MotifeTargets {
    let p1 = motive.points[0].price;
    let p2 = motive.points[1].price;
    let p3 = motive.points[2].price;
    let p4 = motive.points[3].price;

    let w1 = (p2 - p1).abs();
    let w3 = (p4 - p3).abs();

    let base = if motive.direction > 0 {
        p4 // Wave 4 end (low in bullish)
    } else {
        p4 // Wave 4 end (high in bearish)
    };

    let direction = motive.direction as f64;

    MotifeTargets {
        target_1_0: base + (direction * w1),
        target_1_272: base + (direction * w1 * 1.272),
        target_1_618: base + (direction * w1 * 1.618),
        target_0_5_w3: base + (direction * w3 * 0.5),
    }
}

/// Project wave C targets from wave A magnitude.
pub fn project_corrective_targets(corr: &CorrectiveWave) -> CorrectiveTargets {
    let pa = corr.points[0].price;
    let pb = corr.points[1].price;

    let a_move = (pb - pa).abs();
    let base = pb;
    let direction = corr.direction as f64;

    CorrectiveTargets {
        target_1_0: base + (direction * a_move),
        target_1_272: base + (direction * a_move * 1.272),
        target_1_618: base + (direction * a_move * 1.618),
    }
}

/// Most likely target (heuristic: 1.618× is most common).
pub fn motive_primary_target(targets: &MotifeTargets) -> f64 {
    targets.target_1_618
}

/// Most likely target for corrective wave.
pub fn corrective_primary_target(targets: &CorrectiveTargets) -> f64 {
    targets.target_1_618
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::luxalgo_zigzag::ZigZagPoint;

    fn mock_point(bars_ago: usize, price: f64, direction: i8) -> ZigZagPoint {
        ZigZagPoint {
            bars_ago,
            price,
            direction,
        }
    }

    #[test]
    fn test_motive_targets_bullish() {
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

        let targets = project_motive_targets(&motive);
        assert!(targets.target_1_0 > 105.0); // Should be above p4
        assert!(targets.target_1_618 > targets.target_1_0);
    }

    #[test]
    fn test_corrective_targets() {
        let corr = CorrectiveWave {
            points: [
                mock_point(2, 100.0, 1),
                mock_point(1, 95.0, -1),
                mock_point(0, 85.0, 1),
            ],
            direction: -1,
            score: 0.7,
        };

        let targets = project_corrective_targets(&corr);
        // Direction is -1 (downward), so targets should be below 95
        assert!(targets.target_1_0 < 95.0);
        assert!(targets.target_1_618 < targets.target_1_0);
    }

    #[test]
    fn test_primary_targets() {
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

        let targets = project_motive_targets(&motive);
        let primary = motive_primary_target(&targets);
        assert_eq!(primary, targets.target_1_618);
    }
}
