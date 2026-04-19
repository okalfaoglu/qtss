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

    /// Scan *all* 5-pivot windows (sliding window) and return the
    /// highest-scoring detection. The original implementation only
    /// checked `pivots[len-5..]` which almost never aligns with a
    /// harmonic ratio set by chance — the sliding scan makes detection
    /// practical on real data.
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

        let mut global_best: Option<(usize, &HarmonicSpec, Direction, f64)> = None;

        for start in 0..=(pivots.len() - 5) {
            let window = &pivots[start..start + 5];
            let direction = match window[0].kind {
                PivotKind::Low => Direction::Bullish,
                PivotKind::High => Direction::Bearish,
            };
            let Some(pts) = collect_points(window, matches!(direction, Direction::Bearish)) else {
                continue;
            };
            for spec in PATTERNS {
                if let Some(score) = match_pattern(spec, &pts, self.config.global_slack) {
                    if global_best.as_ref().map(|(_, _, _, s)| score > *s).unwrap_or(true) {
                        global_best = Some((start, spec, direction, score));
                    }
                }
            }
        }

        let (start, spec, direction, score) = global_best?;
        if (score as f32) < self.config.min_structural_score {
            return None;
        }

        let window = &pivots[start..start + 5];
        let kind = PatternKind::Harmonic(format!(
            "{}_{}",
            spec.name,
            match direction {
                Direction::Bullish => "bull",
                Direction::Bearish => "bear",
            }
        ));
        let anchors = label_anchors(window, self.config.pivot_level);

        // Invalidation = the price level where the pattern breaks.
        // For extension patterns (butterfly, crab) where D extends beyond X:
        //   Bull: invalidation = below D (further extension would break pattern)
        //   Bear: invalidation = above D (further extension would break pattern)
        // For retracement patterns (gartley, bat) where D stays between X and A:
        //   Bull: invalidation = below X (break of structure start)
        //   Bear: invalidation = above X (break of structure start)
        let d_price = window[4].price;
        let x_price = window[0].price;
        // Explicit per-spec flag — AD alone isn't enough to classify
        // 5-0 (AD near zero but invalidation is still D-anchored).
        let is_extension = spec.extension;
        let invalidation_price = match (direction, is_extension) {
            // Extension patterns: SL beyond D with tight buffer.
            // Standard harmonic practice: 1-2% of XA beyond D.
            (Direction::Bullish, true) => {
                let xa = (window[1].price - x_price).abs();
                let buffer = Decimal::from_f64_retain(0.02)
                    .unwrap_or(Decimal::ZERO) * xa;
                d_price - buffer // SL below D
            }
            (Direction::Bearish, true) => {
                let xa = (window[1].price - x_price).abs();
                let buffer = Decimal::from_f64_retain(0.02)
                    .unwrap_or(Decimal::ZERO) * xa;
                d_price + buffer // SL above D
            }
            // Retracement patterns: SL at X (start of pattern).
            (Direction::Bullish, false) => x_price,
            (Direction::Bearish, false) => x_price,
        };

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
