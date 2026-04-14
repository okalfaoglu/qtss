//! WyckoffDetector — runs every event spec through the same loop and
//! emits **every** detection whose score clears the
//! `min_structural_score` gate (P13).
//!
//! **Why not a single "best" detection anymore?** Wyckoff phases require
//! a *vocabulary* of distinct events (PS, SC, AR, ST, UA, STB, Spring,
//! SOS, LPS, JAC, BUEC …) to advance A→B→C→D→E through the sequential
//! gates in `WyckoffStructureTracker::try_advance_phase`. When this
//! detector returned only the top-scoring match per call, the SC event
//! almost always shadowed UA / SOS / LPS on the same pivot window — so
//! downstream the tracker saw nothing but a wall of SC detections and
//! could never collect the evidence needed to transition out of Phase A.
//! That is the root cause of the "0 A→B→C→D→E cycles in 4 years"
//! finding on BTC 1d. We now return every qualifying match; the
//! orchestrator dedups per (symbol, TF, subkind, anchor) via
//! `anchor_already_seen`.
//!
//! Specs that need extra pivots beyond the configured
//! `min_range_pivots` (e.g. Spring, which consumes the trailing range
//! *plus* one false-break pivot) are sized by each `eval` itself; the
//! detector just hands them the full tail.

use crate::config::WyckoffConfig;
use crate::error::WyckoffResult;
use crate::events::EVENTS;
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
    ) -> Vec<Detection> {
        let pivots = tree.at_level(self.config.pivot_level);
        if pivots.len() < self.config.min_range_pivots {
            return Vec::new();
        }

        let mut out = Vec::new();
        for spec in EVENTS {
            let Some(m) = (spec.eval)(pivots, &self.config) else { continue };
            if (m.score as f32) < self.config.min_structural_score {
                continue;
            }
            let kind = PatternKind::Wyckoff(format!("{}_{}", spec.name, m.variant));
            let anchors = label_anchors(pivots, &m.anchor_labels, self.config.pivot_level);
            out.push(Detection::new(
                instrument.clone(),
                timeframe,
                kind,
                PatternState::Forming,
                anchors,
                m.score as f32,
                m.invalidation,
                regime.clone(),
            ));
        }
        out
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
