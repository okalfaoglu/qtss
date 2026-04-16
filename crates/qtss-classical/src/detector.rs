//! ClassicalDetector — runs every shape spec through the same loop and
//! emits the highest-scoring detection (if any).

use crate::config::ClassicalConfig;
use crate::error::ClassicalResult;
use crate::shapes::{ShapeMatch, SHAPES, SHAPES_WITH_BARS};
use qtss_domain::v2::bar::Bar;
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
        self.detect_with_bars(tree, &[], instrument, timeframe, regime)
    }

    /// P5.2 — bar-aware variant. Evaluates bar-less shapes (SHAPES) and
    /// bar-aware shapes (SHAPES_WITH_BARS) and returns the single best.
    /// When `bars` is empty only the pivot-only shapes are considered, so
    /// callers without bar data can still use this path safely.
    pub fn detect_with_bars(
        &self,
        tree: &PivotTree,
        bars: &[Bar],
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Option<Detection> {
        let pivots = tree.at_level(self.config.pivot_level);

        // Best = (name, pivots_needed, anchor_labels, ShapeMatch)
        let mut best: Option<(&'static str, usize, ShapeMatch)> = None;
        let mut consider = |name: &'static str, needed: usize, m: ShapeMatch| {
            if best.as_ref().map(|(_, _, b)| m.score > b.score).unwrap_or(true) {
                best = Some((name, needed, m));
            }
        };

        for spec in SHAPES {
            if pivots.len() < spec.pivots_needed {
                continue;
            }
            let tail = &pivots[pivots.len() - spec.pivots_needed..];
            if let Some(m) = (spec.eval)(tail, &self.config) {
                consider(spec.name, spec.pivots_needed, m);
            }
        }
        for spec in SHAPES_WITH_BARS {
            if pivots.len() < spec.pivots_needed || bars.len() < spec.bars_needed {
                continue;
            }
            let tail = &pivots[pivots.len() - spec.pivots_needed..];
            if let Some(m) = (spec.eval)(tail, bars, &self.config) {
                consider(spec.name, spec.pivots_needed, m);
            }
        }

        let (name, needed, m) = best?;
        if (m.score as f32) < self.config.min_structural_score {
            return None;
        }

        let tail = &pivots[pivots.len() - needed..];
        let kind = PatternKind::Classical(format!("{}_{}", name, m.variant));
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
