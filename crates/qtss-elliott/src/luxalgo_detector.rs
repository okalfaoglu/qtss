//! LuxAlgo Elliott Wave detector — motive + corrective patterns from pivot stream.
//!
//! Scans a PivotTree level and emits:
//!   - 5-wave impulse (6 pivots: 0-1-2-3-4-5)
//!   - 3-wave correction (4 pivots: 0-A-B-C)

use crate::common::label_anchors;
use crate::config::ElliottConfig;
use crate::corrective::detect_corrective;
use crate::formation::FormationDetector;
use crate::luxalgo_zigzag::ZigZagPoint;
use crate::motive::detect_motive;
use qtss_domain::v2::detection::{Detection, PatternKind, PatternState};
use qtss_domain::v2::instrument::Instrument;
use qtss_domain::v2::pivot::{PivotKind, PivotTree};
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;
use rust_decimal::prelude::ToPrimitive;

const MOTIVE_ANCHORS: usize = 6;
const CORRECTIVE_ANCHORS: usize = 4;
const MOTIVE_LABELS: &[&str] = &["0", "1", "2", "3", "4", "5"];
const CORRECTIVE_LABELS: &[&str] = &["0", "A", "B", "C"];

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
        if pivots.len() < CORRECTIVE_ANCHORS {
            return Vec::new();
        }

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

        let mut results = Vec::new();

        if pivots.len() >= MOTIVE_ANCHORS {
            for start in 0..=(points.len() - MOTIVE_ANCHORS) {
                let window = &points[start..start + MOTIVE_ANCHORS];
                let Some(motive) = detect_motive(window) else { continue };
                if (motive.score as f32) < self.config.min_structural_score {
                    continue;
                }
                let suffix = if motive.direction > 0 { "bull" } else { "bear" };
                let subkind = format!("luxalgo_impulse_{suffix}");
                let pivot_window = &pivots[start..start + MOTIVE_ANCHORS];
                let anchors =
                    label_anchors(pivot_window, self.config.pivot_level, MOTIVE_LABELS);
                let invalidation_price = pivot_window[0].price;

                results.push(Detection::new(
                    instrument.clone(),
                    timeframe,
                    PatternKind::Elliott(subkind),
                    PatternState::Forming,
                    anchors,
                    motive.score as f32,
                    invalidation_price,
                    regime.clone(),
                ));
            }
        }

        for start in 0..=(points.len() - CORRECTIVE_ANCHORS) {
            let window = &points[start..start + CORRECTIVE_ANCHORS];
            let Some(corr) = detect_corrective(window) else { continue };
            if (corr.score as f32) < self.config.min_structural_score {
                continue;
            }
            let suffix = if corr.direction > 0 { "up" } else { "down" };
            let subkind = format!("luxalgo_correction_{suffix}");
            let pivot_window = &pivots[start..start + CORRECTIVE_ANCHORS];
            let anchors =
                label_anchors(pivot_window, self.config.pivot_level, CORRECTIVE_LABELS);
            let invalidation_price = pivot_window[0].price;

            results.push(Detection::new(
                instrument.clone(),
                timeframe,
                PatternKind::Elliott(subkind),
                PatternState::Forming,
                anchors,
                corr.score as f32,
                invalidation_price,
                regime.clone(),
            ));
        }

        results
    }
}
