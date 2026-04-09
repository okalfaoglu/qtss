//! Flat correction (A-B-C, internal 3-3-5).
//!
//! A flat is a sideways three-wave correction. Distinguishing feature
//! vs a zigzag: wave B retraces nearly all of wave A (≥ ~90 %), and
//! wave C ends at-or-near the end of wave A.
//!
//! Sub-types per Frost & Prechter:
//!
//!   * **Regular** flat — B ≈ 100 % of A, C ≈ 100 % of A (small
//!     overshoot allowed). Subkind: `flat_regular_<dir>`.
//!   * **Expanded** flat — B *exceeds* the start of A (> ~105 %), C
//!     extends well beyond the end of A (~1.272–1.618 × A). Most
//!     common in liquid markets. Subkind: `flat_expanded_<dir>`.
//!   * **Running** flat — B exceeds 100 % but C *fails* to reach the
//!     end of A. Rare; signals very strong underlying trend. Subkind:
//!     `flat_running_<dir>`.
//!
//! Pivot count: 4 (start, end-A, end-B, end-C). Direction is decided
//! by the *correction* leg — a flat that corrects a bullish leg starts
//! at a HIGH, so its `dir` suffix is `bear`.
//!
//! Validity rules:
//!   1. Strict alternation of pivot kinds.
//!   2. A leg ≠ 0 and C leg in the same sign as A.
//!   3. B ≥ 0.85 × |A| (otherwise it's a zigzag candidate).
//!   4. The C/A ratio decides the sub-type via a small dispatch table.

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

const ANCHOR_LABELS: &[&str] = &["0", "A", "B", "C"];
const B_REFS: &[f64] = &[1.0, 1.05, 1.272];
const C_REGULAR_REFS: &[f64] = &[1.0, 1.05];
const C_EXPANDED_REFS: &[f64] = &[1.272, 1.618];

/// Sub-type dispatch — pure look-up, no scattered if/else (CLAUDE.md #1).
/// Each row: (label, min_b_ratio, min_c_ratio, max_c_ratio).
const SUBTYPES: &[(&str, f64, f64, f64)] = &[
    // running: B overshoots strongly, C fails to reach A end (c<1.0).
    ("running", 1.05, 0.0, 1.0),
    // expanded: B overshoots, C extends well past A end.
    ("expanded", 1.05, 1.05, 2.0),
    // regular: B near 100% of A, C near 100% of A.
    ("regular", 0.85, 0.85, 1.10),
];

pub struct FlatDetector {
    config: ElliottConfig,
}

impl FlatDetector {
    pub fn new(config: ElliottConfig) -> ElliottResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }
}

impl FormationDetector for FlatDetector {
    fn name(&self) -> &'static str {
        "flat"
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
        let raw: Vec<f64> = tail.iter().map(|p| p.price.to_f64().unwrap_or(0.0)).collect();

        let a_leg = raw[1] - raw[0];
        let b_leg = raw[2] - raw[1];
        let c_leg = raw[3] - raw[2];
        if a_leg == 0.0 || c_leg == 0.0 {
            return Vec::new();
        }
        if a_leg.signum() != c_leg.signum() {
            return Vec::new();
        }
        if b_leg.signum() == a_leg.signum() {
            return Vec::new();
        }

        let a_abs = a_leg.abs();
        let b_ratio = b_leg.abs() / a_abs;
        // C measured against A's end → tail[1]. Positive ratio if C
        // continues in A's direction past tail[1].
        let c_distance = (raw[3] - raw[1]) * a_leg.signum();
        let c_ratio_signed = c_distance / a_abs;

        // Sub-type look-up — first row that matches wins.
        let subtype = SUBTYPES.iter().find_map(|(label, b_min, c_min, c_max)| {
            if b_ratio >= *b_min && c_ratio_signed >= *c_min - 1.0 && c_ratio_signed <= *c_max - 0.0 {
                Some(*label)
            } else {
                None
            }
        });
        let Some(subtype) = subtype else {
            return Vec::new();
        };

        // Score: how clean is B retrace + how close C ratio sits to its
        // canonical target for this sub-type.
        let s_b = nearest_fib_score(b_ratio, B_REFS);
        let c_refs = if subtype == "expanded" {
            C_EXPANDED_REFS
        } else {
            C_REGULAR_REFS
        };
        let s_c = nearest_fib_score(c_ratio_signed.max(0.0), c_refs);
        let score = mean_score(&[s_b, s_c]);

        if (score as f32) < self.config.min_structural_score {
            return Vec::new();
        }

        // The correction's "dir" reflects which way it pushes price.
        // A negative a_leg → downward correction → bear suffix.
        let suffix = if a_leg < 0.0 { "bear" } else { "bull" };
        let subkind = format!("flat_{subtype}_{suffix}");
        let anchors = label_anchors(tail, self.config.pivot_level, ANCHOR_LABELS);
        let projected =
            projection::project(&subkind, &anchors, self.config.pivot_level);
        let sub_waves = decomposition::decompose(tree, &anchors, self.config.pivot_level);
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
