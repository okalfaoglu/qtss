//! LuxAlgo Elliott Wave detector — motive + corrective patterns from pivot stream.
//!
//! Integrates `motive::detect_motive` and `corrective::detect_corrective`
//! into the `FormationDetector` pipeline. Scans PivotTree for:
//!   - 5-wave impulse (1-2-3-4-5)
//!   - 3-wave correction (A-B-C)

use crate::config::ElliottConfig;
use crate::corrective::detect_corrective;
use crate::formation::FormationDetector;
use crate::motive::detect_motive;
use crate::zigzag::ZigZagPoint;
use qtss_domain::v2::detection::{Detection, PatternKind, PatternState};
use qtss_domain::v2::instrument::Instrument;
use qtss_domain::v2::pivot::{PivotKind, PivotTree};
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;

pub struct LuxAlgoDetector {
    config: ElliottConfig,
}

impl LuxAlgoDetector {
    pub fn new(config: ElliottConfig) -> Self {
        Self { config }
    }
}

impl FormationDetector for LuxAlgoDetector {
    fn name(&self) -> &'static str {
        "luxalgo"
    }

    fn detect(
        &self,
        tree: &PivotTree,
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        let pivots = tree.at_level(self.config.pivot_level);
        if pivots.len() < 3 {
            return Vec::new();
        }

        let mut results = Vec::new();

        // Convert pivots to ZigZagPoints for pattern detection.
        let points: Vec<ZigZagPoint> = pivots
            .iter()
            .enumerate()
            .map(|(i, p)| ZigZagPoint {
                bars_ago: pivots.len() - 1 - i,
                price: p.price.to_f64().unwrap_or(0.0),
                direction: match p.kind {
                    PivotKind::High => 1,
                    PivotKind::Low => -1,
                },
            })
            .collect();

        // Detect 5-wave motive patterns (scan all 5-pivot windows).
        if pivots.len() >= 5 {
            for start in 0..=(points.len() - 5) {
                let window = &points[start..start + 5];
                if let Some(motive) = detect_motive(window) {
                    let suffix = if motive.direction > 0 {
                        "bull"
                    } else {
                        "bear"
                    };
                    let subkind = format!("luxalgo_impulse_{suffix}");

                    let anchors = window
                        .iter()
                        .enumerate()
                        .map(|(i, pt)| {
                            let label = match i {
                                0 => "0",
                                1 => "1",
                                2 => "2",
                                3 => "3",
                                4 => "4",
                                _ => "?",
                            };
                            (label.to_string(), pt.bars_ago)
                        })
                        .collect();

                    results.push(
                        Detection::new(
                            instrument.clone(),
                            timeframe,
                            PatternKind::Elliott(subkind),
                            PatternState::Forming,
                            anchors,
                            motive.score as f32,
                            pivots.get(start).map(|p| p.price).unwrap_or_default(),
                            regime.clone(),
                        ),
                    );
                }
            }
        }

        // Detect 3-wave corrective patterns (scan all 3-pivot windows).
        if pivots.len() >= 3 {
            for start in 0..=(points.len() - 3) {
                let window = &points[start..start + 3];
                if let Some(corr) = detect_corrective(window) {
                    let suffix = if corr.direction > 0 {
                        "up"
                    } else {
                        "down"
                    };
                    let subkind = format!("luxalgo_correction_{suffix}");

                    let anchors = window
                        .iter()
                        .enumerate()
                        .map(|(i, pt)| {
                            let label = match i {
                                0 => "A",
                                1 => "B",
                                2 => "C",
                                _ => "?",
                            };
                            (label.to_string(), pt.bars_ago)
                        })
                        .collect();

                    results.push(
                        Detection::new(
                            instrument.clone(),
                            timeframe,
                            PatternKind::Elliott(subkind),
                            PatternState::Forming,
                            anchors,
                            corr.score as f32,
                            pivots.get(start).map(|p| p.price).unwrap_or_default(),
                            regime.clone(),
                        ),
                    );
                }
            }
        }

        results
    }
}
