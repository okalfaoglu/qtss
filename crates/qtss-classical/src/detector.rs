//! ClassicalDetector — runs every shape spec through the same loop and
//! emits the highest-scoring detection (if any).

use crate::config::ClassicalConfig;
use crate::error::ClassicalResult;
use crate::shapes::{ShapeMatch, ShapeSpec, SHAPES};
use qtss_domain::v2::detection::{Detection, PatternKind, PatternState, PivotRef};
use qtss_domain::v2::instrument::Instrument;
use qtss_domain::v2::pivot::{Pivot, PivotLevel, PivotTree};
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;

pub struct ClassicalDetector {
    config: ClassicalConfig,
}

impl ClassicalDetector {
    pub fn new(config: ClassicalConfig) -> ClassicalResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &ClassicalConfig {
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

        // Walk every spec, evaluate against its required tail, keep best.
        let mut best: Option<(&ShapeSpec, ShapeMatch)> = None;
        for spec in SHAPES {
            if pivots.len() < spec.pivots_needed {
                continue;
            }
            let tail = &pivots[pivots.len() - spec.pivots_needed..];
            if let Some(m) = (spec.eval)(tail, &self.config) {
                if best
                    .as_ref()
                    .map(|(_, b)| m.score > b.score)
                    .unwrap_or(true)
                {
                    best = Some((spec, m));
                }
            }
        }
        let (spec, m) = best?;
        if (m.score as f32) < self.config.min_structural_score {
            return None;
        }

        let tail = &pivots[pivots.len() - spec.pivots_needed..];
        let kind = PatternKind::Classical(format!("{}_{}", spec.name, m.variant));
        let anchors = label_anchors(tail, &m.anchor_labels, self.config.pivot_level);

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

fn label_anchors(tail: &[Pivot], labels: &[&'static str], level: PivotLevel) -> Vec<PivotRef> {
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
