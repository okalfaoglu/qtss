//! Zigzag (A-B-C, 5-3-5) corrective wave detector.
//!
//! Per Frost & Prechter: a zigzag is a sharp three-wave correction
//! whose internal structure is 5-3-5. From a *pivot* point of view we
//! see four extremes: the start, the end of A, the end of B, and the
//! end of C. Direction is decided by the first pivot:
//!   * starts at a HIGH → downward zigzag (corrects a bullish leg)
//!   * starts at a LOW  → upward zigzag (corrects a bearish leg)
//!
//! Validity rules (after normalization to bullish-positive frame):
//!   1. Strict alternation of pivot kinds (4 pivots).
//!   2. B retraces no more than 0.786 of A — beyond that the structure
//!      is more likely a flat or expanded flat.
//!   3. C extends *beyond* the end of A in A's direction (otherwise
//!      it's a truncated/failed correction, handled by `flat`).
//!
//! Structural score combines:
//!   * Proximity of B-retrace to canonical {0.5, 0.618, 0.786}.
//!   * Proximity of C-extension (vs A) to {1.0, 1.272, 1.618}.

use crate::common::{alternation_ok, label_anchors, mean_score, nearest_fib_score};
use crate::config::ElliottConfig;
use crate::error::ElliottResult;
use crate::formation::FormationDetector;
use crate::decomposition;
use crate::projection;
use qtss_domain::v2::detection::{Detection, PatternKind, PatternState};
use qtss_domain::v2::instrument::Instrument;
use qtss_domain::v2::pivot::PivotTree;
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;
use rust_decimal::prelude::ToPrimitive;

const B_REFS: &[f64] = &[0.5, 0.618, 0.786];
const C_REFS: &[f64] = &[1.0, 1.272, 1.618];
const ANCHOR_LABELS: &[&str] = &["0", "A", "B", "C"];

pub struct ZigzagDetector {
    config: ElliottConfig,
}

impl ZigzagDetector {
    pub fn new(config: ElliottConfig) -> ElliottResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }
}

impl FormationDetector for ZigzagDetector {
    fn name(&self) -> &'static str {
        "zigzag"
    }

    fn detect(
        &self,
        tree: &PivotTree,
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        let pivots = tree.at_level(self.config.pivot_level);
        if pivots.len() < 4 {
            return Vec::new();
        }
        let tail = &pivots[pivots.len() - 4..];
        if !alternation_ok(tail) {
            return Vec::new();
        }
        // Zigzag is short enough (4 pivots) that the dir-normalization
        // trick from impulses adds no value — read the raw prices and
        // reason about leg signs directly.
        let raw: Vec<f64> = tail
            .iter()
            .map(|q| q.price.to_f64().unwrap_or(0.0))
            .collect();

        let a_leg = raw[1] - raw[0];
        let b_leg = raw[2] - raw[1];
        let c_leg = raw[3] - raw[2];

        // A and C must move in the same direction; B must oppose.
        if a_leg == 0.0 || b_leg == 0.0 || c_leg == 0.0 {
            return Vec::new();
        }
        if a_leg.signum() != c_leg.signum() {
            return Vec::new();
        }
        if b_leg.signum() == a_leg.signum() {
            return Vec::new();
        }

        let a_abs = a_leg.abs();
        let b_abs = b_leg.abs();
        let c_abs = c_leg.abs();

        let b_retrace = b_abs / a_abs;
        if b_retrace > 0.95 {
            // Almost full retrace → flat territory, not a zigzag.
            return Vec::new();
        }

        // C must extend beyond the end of A — i.e. tail[3] is past tail[1]
        // in A's direction.
        let c_beyond_a = if a_leg < 0.0 {
            raw[3] < raw[1]
        } else {
            raw[3] > raw[1]
        };
        if !c_beyond_a {
            return Vec::new();
        }

        let c_ext = c_abs / a_abs;
        let s_b = nearest_fib_score(b_retrace, B_REFS);
        let s_c = nearest_fib_score(c_ext, C_REFS);
        let score = mean_score(&[s_b, s_c]);

        if (score as f32) < self.config.min_structural_score {
            return Vec::new();
        }

        // Subkind: a downward zigzag (a_leg<0) corrects a prior bullish
        // leg → tag it `_bear` (the correction itself is bearish). An
        // upward zigzag → `_bull`.
        let suffix = if a_leg < 0.0 { "bear" } else { "bull" };
        let subkind = format!("zigzag_abc_{suffix}");
        let anchors = label_anchors(tail, self.config.pivot_level, ANCHOR_LABELS);
        let projected =
            projection::project(&subkind, &anchors, self.config.pivot_level);
        let sub_waves = decomposition::decompose(tree, &anchors, self.config.pivot_level);
        // Invalidation: the start of A. If price reverses past it the
        // correction count is broken.
        let invalidation_price = tail[0].price;

        vec![Detection::new(
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
        .with_sub_waves(sub_waves)]
    }
}
