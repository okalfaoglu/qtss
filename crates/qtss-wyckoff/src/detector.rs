//! WyckoffDetector — runs every event spec through the same loop and
//! emits the highest-scoring detection (if any). Specs that need extra
//! pivots beyond the configured `min_range_pivots` (e.g. Spring, which
//! consumes the trailing range *plus* one false-break pivot) are sized
//! by each `eval` itself; the detector just hands them the full tail.

use crate::config::WyckoffConfig;
use crate::error::WyckoffResult;
use crate::events::{EventMatch, EventSpec, EVENTS};
use qtss_domain::v2::detection::{Detection, PatternKind, PatternState, PivotRef};
use qtss_domain::v2::instrument::Instrument;
use qtss_domain::v2::pivot::{Pivot, PivotLevel, PivotTree};
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;

pub struct WyckoffDetector {
    config: WyckoffConfig,
}

impl WyckoffDetector {
    pub fn new(config: WyckoffConfig) -> WyckoffResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &WyckoffConfig {
        &self.config
    }

    pub fn detect(
        &self,
        tree: &PivotTree,
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Option<Detection> {
        let pivots = tree.at_level(self.config.pivot_level);
        if pivots.len() < self.config.min_range_pivots {
            return None;
        }

        let mut best: Option<(&EventSpec, EventMatch)> = None;
        for spec in EVENTS {
            if let Some(m) = (spec.eval)(pivots, &self.config) {
                if best.as_ref().map(|(_, b)| m.score > b.score).unwrap_or(true) {
                    best = Some((spec, m));
                }
            }
        }
        let (spec, m) = best?;
        if (m.score as f32) < self.config.min_structural_score {
            return None;
        }

        let kind = PatternKind::Wyckoff(format!("{}_{}", spec.name, m.variant));
        let anchors = label_anchors(pivots, &m.anchor_labels, self.config.pivot_level);
        Some(Detection::new(
            instrument.clone(),
            timeframe,
            kind,
            PatternState::Forming,
            anchors,
            m.score as f32,
            m.invalidation,
            regime.clone(),
        ))
    }
}

fn label_anchors(
    pivots: &[Pivot],
    labels: &[&'static str],
    level: PivotLevel,
) -> Vec<PivotRef> {
    let take = labels.len().min(pivots.len());
    let tail = &pivots[pivots.len() - take..];
    tail.iter()
        .zip(labels.iter())
        .map(|(p, l)| PivotRef {
            bar_index: p.bar_index,
            price: p.price,
            level,
            label: Some((*l).to_string()),
        })
        .collect()
}
