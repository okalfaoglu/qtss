//! Triangle correction (A-B-C-D-E, internal 3-3-3-3-3).
//!
//! Triangles are five-leg sideways consolidations whose touch points
//! trace two converging (or diverging) trendlines. From a pivot tape we
//! see six extremes: the start of A and the ends of A/B/C/D/E.
//!
//! Sub-types per Frost & Prechter:
//!
//!   * **Contracting** — each successive same-side leg is *smaller*
//!     than the previous one. Both trendlines converge. Most common.
//!     Subkind: `triangle_contracting_<dir>`.
//!   * **Expanding** (broadening) — each successive same-side leg is
//!     *larger*. Trendlines diverge. Rare; signals climactic
//!     volatility. Subkind: `triangle_expanding_<dir>`.
//!   * **Barrier** — one trendline is roughly horizontal (the "barrier"
//!     side), the other contracts into it. Subkind:
//!     `triangle_barrier_<dir>`.
//!
//! Validity rules (after raw-price reading; triangles are short enough
//! that direction normalization adds no value):
//!   1. Strict alternation of pivot kinds across all 6 pivots.
//!   2. The two same-side legs (A→C, C→E or B→D) move in opposite
//!      directions to the alternating ones.
//!   3. The shape classifies into exactly one sub-type via a small
//!      dispatch table — first match wins.
//!
//! Direction tag: triangles correct an existing leg, so a triangle
//! that starts at a HIGH (downward A) is correcting a bullish leg →
//! `_bear` suffix; vice versa.

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

const ANCHOR_LABELS: &[&str] = &["0", "A", "B", "C", "D", "E"];
/// Tolerance for "horizontal" trendline in barrier triangles — within
/// 5% considered flat.
const BARRIER_FLAT_TOL: f64 = 0.05;

/// Triangle sub-type classifier — pure look-up table dispatched on the
/// signed contraction ratios of the two trendlines (top vs bottom).
/// Each entry: (label, predicate).
type Predicate = fn(top_ratio: f64, bot_ratio: f64) -> bool;
const SUBTYPES: &[(&str, Predicate)] = &[
    ("contracting", is_contracting),
    ("expanding", is_expanding),
    ("barrier", is_barrier),
];

fn is_contracting(top_ratio: f64, bot_ratio: f64) -> bool {
    top_ratio < 1.0 && bot_ratio < 1.0
}

fn is_expanding(top_ratio: f64, bot_ratio: f64) -> bool {
    top_ratio > 1.0 && bot_ratio > 1.0
}

fn is_barrier(top_ratio: f64, bot_ratio: f64) -> bool {
    let top_flat = (top_ratio - 1.0).abs() < BARRIER_FLAT_TOL;
    let bot_flat = (bot_ratio - 1.0).abs() < BARRIER_FLAT_TOL;
    (top_flat && bot_ratio < 1.0) || (bot_flat && top_ratio < 1.0)
}

pub struct TriangleDetector {
    config: ElliottConfig,
}

impl TriangleDetector {
    pub fn new(config: ElliottConfig) -> ElliottResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }
}

impl FormationDetector for TriangleDetector {
    fn name(&self) -> &'static str {
        "triangle"
    }

    fn detect(
        &self,
        tree: &PivotTree,
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        let pivots = tree.at_level(self.config.pivot_level);
        if pivots.len() < 6 {
            return Vec::new();
        }
        let tail = &pivots[pivots.len() - 6..];
        if !alternation_ok(tail) {
            return Vec::new();
        }
        let raw: Vec<f64> = tail
            .iter()
            .map(|p| p.price.to_f64().unwrap_or(0.0))
            .collect();

        // Six pivots → five legs. Tag the touchpoints by side: pivots
        // 1, 3, 5 are one trendline, pivots 2, 4 are the other. The
        // first leg's sign decides which side is "top" (highs) and
        // which is "bottom" (lows).
        let first_leg = raw[1] - raw[0];
        if first_leg == 0.0 {
            return Vec::new();
        }
        // For a triangle starting at a HIGH (first_leg < 0): pivots
        // 0, 2, 4 are highs (top trendline) and 1, 3, 5 are lows.
        // For one starting at a LOW: swapped.
        let (tops, bots): (Vec<f64>, Vec<f64>) = if first_leg < 0.0 {
            (vec![raw[0], raw[2], raw[4]], vec![raw[1], raw[3], raw[5]])
        } else {
            (vec![raw[1], raw[3], raw[5]], vec![raw[0], raw[2], raw[4]])
        };

        // Per-side leg comparisons. Top: |t1-t0| vs |t2-t1|.
        // Triangle classification cares about the *ratio* of the second
        // span to the first on each side (>1 expands, <1 contracts).
        let top_first = (tops[1] - tops[0]).abs();
        let top_second = (tops[2] - tops[1]).abs();
        let bot_first = (bots[1] - bots[0]).abs();
        let bot_second = (bots[2] - bots[1]).abs();
        if top_first == 0.0 || bot_first == 0.0 {
            return Vec::new();
        }
        let top_ratio = top_second / top_first;
        let bot_ratio = bot_second / bot_first;

        // Sub-type look-up — first matching predicate wins.
        let subtype = SUBTYPES
            .iter()
            .find_map(|(label, pred)| pred(top_ratio, bot_ratio).then_some(*label));
        let Some(subtype) = subtype else {
            return Vec::new();
        };

        // Score: how cleanly the legs alternate, plus distance of each
        // ratio from canonical contraction (~0.618) for contracting
        // triangles. For expanding/barrier we just take the geometric
        // signal at face value.
        let s_top = nearest_fib_score(top_ratio, &[0.618, 1.0, 1.618]);
        let s_bot = nearest_fib_score(bot_ratio, &[0.618, 1.0, 1.618]);
        let score = mean_score(&[s_top, s_bot]);

        if (score as f32) < self.config.min_structural_score {
            return Vec::new();
        }

        // dir suffix: triangle that starts at a HIGH corrects a bullish
        // leg → bear; vice versa.
        let suffix = if first_leg < 0.0 { "bear" } else { "bull" };
        let subkind = format!("triangle_{subtype}_{suffix}");
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
