//! Impulse detector — scans the most recent 6 pivots at the configured
//! level and emits a [`Detection`] when an impulse passes every rule.
//!
//! The detector is stateless: each call reads the snapshot it's given,
//! makes its decision, and returns. State (last-seen pivot id, dedup,
//! ...) belongs to the wiring layer, not here.

use crate::config::ElliottConfig;
use crate::error::ElliottResult;
use crate::fibs::{proximity_score, WAVE2_REFS, WAVE3_REFS, WAVE4_REFS};
use crate::decomposition;
use crate::projection;
use crate::rules::{ImpulsePoints, RULES};
use qtss_domain::v2::detection::{
    Detection, PatternKind, PatternState, PivotRef,
};
use qtss_domain::v2::instrument::Instrument;
use qtss_domain::v2::pivot::{Pivot, PivotKind, PivotTree};
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;
use rust_decimal::Decimal;

pub struct ImpulseDetector {
    config: ElliottConfig,
}

impl ImpulseDetector {
    pub fn new(config: ElliottConfig) -> ElliottResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &ElliottConfig {
        &self.config
    }

    /// Scan the latest pivots in the configured level and report a single
    /// impulse if one is present. Returns `None` if there are fewer than
    /// six pivots, alternation is broken, or any rule fails.
    pub fn detect(
        &self,
        tree: &PivotTree,
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Option<Detection> {
        let pivots = tree.at_level(self.config.pivot_level);
        if pivots.len() < 6 {
            return None;
        }
        let tail = &pivots[pivots.len() - 6..];

        // The very first pivot decides direction. Bullish impulses begin
        // with a low; bearish impulses begin with a high.
        let direction = match tail[0].kind {
            PivotKind::Low => Direction::Bullish,
            PivotKind::High => Direction::Bearish,
        };

        let normalized = match direction {
            Direction::Bullish => collect_points(tail, false),
            Direction::Bearish => collect_points(tail, true),
        };
        let arr = normalized.as_f64();

        // Run rules in order; bail on the first failure.
        for rule in RULES {
            if rule(&arr).is_err() {
                return None;
            }
        }

        let score = score_impulse(&arr);
        if (score as f32) < self.config.min_structural_score {
            return None;
        }

        let subkind = match direction {
            Direction::Bullish => "impulse_5_bull".to_string(),
            Direction::Bearish => "impulse_5_bear".to_string(),
        };
        let anchors = label_anchors(tail, self.config.pivot_level);
        let projected =
            projection::project(&subkind, &anchors, self.config.pivot_level);
        let sub_waves = decomposition::decompose(tree, &anchors, self.config.pivot_level);
        let invalidation_price = invalidation_for(direction, tail);

        Some(
            Detection::new(
                instrument.clone(),
                timeframe,
                PatternKind::Elliott(subkind),
                PatternState::Forming,
                anchors,
                score as f32,
                invalidation_price,
                regime.clone(),
            )
            .with_projection(projected)
            .with_sub_waves(sub_waves),
        )
    }
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    Bullish,
    Bearish,
}

fn collect_points(tail: &[Pivot], negate: bool) -> ImpulsePoints {
    let sign = if negate {
        Decimal::NEGATIVE_ONE
    } else {
        Decimal::ONE
    };
    ImpulsePoints {
        p0: tail[0].price * sign,
        p1: tail[1].price * sign,
        p2: tail[2].price * sign,
        p3: tail[3].price * sign,
        p4: tail[4].price * sign,
        p5: tail[5].price * sign,
    }
}

fn score_impulse(p: &[f64; 6]) -> f64 {
    let w1 = p[1] - p[0];
    let w3 = p[3] - p[2];
    let w5 = p[5] - p[4];
    if w1 <= 0.0 || w3 <= 0.0 || w5 <= 0.0 {
        return 0.0;
    }
    let w2_ret = (p[1] - p[2]) / w1;
    let w3_ext = w3 / w1;
    let w4_ret = (p[3] - p[4]) / w3;
    let s2 = proximity_score(w2_ret, WAVE2_REFS);
    let s3 = proximity_score(w3_ext, WAVE3_REFS);
    let s4 = proximity_score(w4_ret, WAVE4_REFS);
    // Mean of the three sub-scores. Equal weighting; the validator can
    // re-weight historically if needed.
    (s2 + s3 + s4) / 3.0
}

fn label_anchors(tail: &[Pivot], level: qtss_domain::v2::pivot::PivotLevel) -> Vec<PivotRef> {
    const LABELS: [&str; 6] = ["0", "1", "2", "3", "4", "5"];
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

fn invalidation_for(direction: Direction, tail: &[Pivot]) -> Decimal {
    // Standard practice: invalidate the impulse if price moves back past
    // the start of wave 1 (point p0). For bullish that's tail[0].price,
    // for bearish the same — direction is encoded in the comparison the
    // risk engine performs.
    let _ = direction;
    tail[0].price
}

