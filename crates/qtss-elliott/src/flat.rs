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
/// Sub-type dispatch — Frost & Prechter rules:
/// - Flat B wave must retrace at least 90% of A (b_min ≥ 0.90).
/// - Running: B overshoots A (>105%), C fails to reach A end.
/// - Expanded: B overshoots, C extends past A end.
/// - Regular: B ≈ 100% of A, C ≈ 100% of A.
const SUBTYPES: &[(&str, f64, f64, f64)] = &[
    ("running", 1.05, 0.0, 1.0),
    ("expanded", 1.05, 1.05, 2.0),
    ("regular", 0.90, 0.85, 1.15),
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

        let mut results = Vec::new();
        for start in 0..=(pivots.len() - 4) {
            let tail = &pivots[start..start + 4];
            if !alternation_ok(tail) {
                continue;
            }
            let raw: Vec<f64> = tail.iter().map(|p| p.price.to_f64().unwrap_or(0.0)).collect();

            let a_leg = raw[1] - raw[0];
            let b_leg = raw[2] - raw[1];
            let c_leg = raw[3] - raw[2];
            if a_leg == 0.0 || c_leg == 0.0 { continue; }
            if a_leg.signum() != c_leg.signum() { continue; }
            if b_leg.signum() == a_leg.signum() { continue; }

            let a_abs = a_leg.abs();
            let b_ratio = b_leg.abs() / a_abs;
            let c_distance = (raw[3] - raw[1]) * a_leg.signum();
            let c_ratio_signed = c_distance / a_abs;

            let subtype = SUBTYPES.iter().find_map(|(label, b_min, c_min, c_max)| {
                if b_ratio >= *b_min && c_ratio_signed >= *c_min - 1.0 && c_ratio_signed <= *c_max - 0.0 {
                    Some(*label)
                } else {
                    None
                }
            });
            let Some(subtype) = subtype else { continue; };

            let s_b = nearest_fib_score(b_ratio, B_REFS);
            let c_refs = if subtype == "expanded" { C_EXPANDED_REFS } else { C_REGULAR_REFS };
            let s_c = nearest_fib_score(c_ratio_signed.max(0.0), c_refs);
            let score = mean_score(&[s_b, s_c]);
            if (score as f32) < self.config.min_structural_score { continue; }

            let suffix = if a_leg < 0.0 { "bear" } else { "bull" };
            let subkind = format!("flat_{subtype}_{suffix}");
            let anchors = label_anchors(tail, self.config.pivot_level, ANCHOR_LABELS);
            let projected = projection::project(&subkind, &anchors, self.config.pivot_level);
            let sub_waves = decomposition::decompose(tree, &anchors, self.config.pivot_level);
            let invalidation_price = tail[0].price;

            results.push(Detection::new(
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
            .with_sub_waves(sub_waves));
        }
        results
    }
}
