//! HarmonicDetector — picks the best-matching pattern for the latest
//! five pivots (X, A, B, C, D) at the configured pivot level.
//!
//! Stateless: each call walks the snapshot, normalises bullish/bearish,
//! runs every pattern through the matcher, and returns the highest
//! scoring detection (if any).

use crate::config::HarmonicConfig;
use crate::error::HarmonicResult;
use crate::matcher::{match_pattern, XabcdPoints};
use crate::patterns::{HarmonicSpec, PATTERNS};
use qtss_domain::v2::detection::{
    Detection, PatternKind, PatternState, PivotRef,
};
use qtss_domain::v2::instrument::Instrument;
use qtss_domain::v2::pivot::{Pivot, PivotKind, PivotLevel, PivotTree};
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

pub struct HarmonicDetector {
    config: HarmonicConfig,
}

impl HarmonicDetector {
    pub fn new(config: HarmonicConfig) -> HarmonicResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &HarmonicConfig {
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
        if pivots.len() < 5 {
            return None;
        }
        let tail = &pivots[pivots.len() - 5..];

        // Bullish XABCD begins with a low at X (so the legs alternate
        // low-high-low-high-low). Bearish is the mirror.
        let direction = match tail[0].kind {
            PivotKind::Low => Direction::Bullish,
            PivotKind::High => Direction::Bearish,
        };
        let pts = collect_points(tail, matches!(direction, Direction::Bearish))?;

        // Walk every pattern through the same loop. Highest score wins.
        let mut best: Option<(&HarmonicSpec, f64)> = None;
        for spec in PATTERNS {
            if let Some(score) = match_pattern(spec, &pts, self.config.global_slack) {
                if best.map(|(_, s)| score > s).unwrap_or(true) {
                    best = Some((spec, score));
                }
            }
        }
        let (spec, score) = best?;
        if (score as f32) < self.config.min_structural_score {
            return None;
        }

        let kind = PatternKind::Harmonic(format!(
            "{}_{}",
            spec.name,
            match direction {
                Direction::Bullish => "bull",
                Direction::Bearish => "bear",
            }
        ));
        let anchors = label_anchors(tail, self.config.pivot_level);
        // Standard: invalidation lives at the X point. A move beyond X
        // breaks the harmonic geometry regardless of direction.
        let invalidation_price = tail[0].price;

        Some(Detection::new(
            instrument.clone(),
            timeframe,
            kind,
            PatternState::Forming,
            anchors,
            score as f32,
            invalidation_price,
            regime.clone(),
        ))
    }
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    Bullish,
    Bearish,
}

fn collect_points(tail: &[Pivot], negate: bool) -> Option<XabcdPoints> {
    let sign = if negate { -1.0 } else { 1.0 };
    Some(XabcdPoints {
        x: tail[0].price.to_f64()? * sign,
        a: tail[1].price.to_f64()? * sign,
        b: tail[2].price.to_f64()? * sign,
        c: tail[3].price.to_f64()? * sign,
        d: tail[4].price.to_f64()? * sign,
    })
}

fn label_anchors(tail: &[Pivot], level: PivotLevel) -> Vec<PivotRef> {
    const LABELS: [&str; 5] = ["X", "A", "B", "C", "D"];
    tail.iter()
        .zip(LABELS.iter())
        .map(|(p, l)| PivotRef {
            bar_index: p.bar_index,
            price: p.price,
            level,
            label: Some((*l).to_string()),
        })
        .collect()
}

// Suppress unused-import warning if Decimal isn't referenced after trim.
const _: fn() = || {
    let _: Decimal = Decimal::ZERO;
};
