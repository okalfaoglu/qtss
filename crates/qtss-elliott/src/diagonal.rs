//! Leading & ending diagonals (wedges).
//!
//! Both diagonals are 5-wave structures that look like impulses but
//! relax two impulse rules:
//!
//!   * Wave 4 *is permitted* to overlap wave 1.
//!   * The whole structure contracts (or expands) — successive waves
//!     of the same direction get *shorter*.
//!
//! Difference between the two:
//!
//!   * **Leading** diagonal — appears as wave 1 of an impulse or wave
//!     A of a zigzag. Internally 5-3-5-3-5. Marks the *start* of a new
//!     directional move.
//!   * **Ending** diagonal — appears as wave 5 of an impulse or wave C
//!     of a zigzag. Internally 3-3-3-3-3. Marks the *end* of an
//!     extended move and warns of reversal.
//!
//! From a pivot-only perspective we cannot tell 5-3-5-3-5 apart from
//! 3-3-3-3-3 (we'd need bar-level sub-counts). The detector therefore
//! disambiguates by *position*: if the wedge fully retraces a prior
//! strong leg, it's a leading diagonal (start of move); if it extends
//! a prior strong leg in the same direction, it's an ending diagonal
//! (climax). When the surrounding context is ambiguous we tag both as
//! `_unknown` and let the validator decide.
//!
//! Validity rules (after normalization to bullish-positive frame):
//!   1. Strict alternation of pivot kinds.
//!   2. Each impulse leg has the right sign (p1>p0, p2<p1, ... p5>p4).
//!   3. Wave 3 strictly shorter than wave 1; wave 5 strictly shorter
//!      than wave 3 (contracting wedge).
//!   4. Wave 4 *may* overlap wave 1 — not enforced as a rule, but the
//!      score rewards overlap because that's the diagnostic signature.
//!
//! Structural score:
//!   * Wedge contraction tightness (how cleanly w3<w1, w5<w3).
//!   * Wave-2 retrace fib proximity.
//!   * Bonus for w4 actually entering w1 territory.

use crate::common::{
    alternation_ok, label_anchors, mean_score, nearest_fib_score, normalize, Direction,
};
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

const W2_REFS: &[f64] = &[0.5, 0.618, 0.786];
const ANCHOR_LABELS: &[&str] = &["0", "1", "2", "3", "4", "5"];

/// Which diagonal flavor this detector emits. Both flavors share the
/// same wedge geometry — only the subkind name differs.
#[derive(Debug, Clone, Copy)]
pub enum DiagonalKind {
    Leading,
    Ending,
}

impl DiagonalKind {
    fn subkind_prefix(self) -> &'static str {
        match self {
            DiagonalKind::Leading => "leading_diagonal_5_3_5",
            DiagonalKind::Ending => "ending_diagonal_3_3_3",
        }
    }
}

pub struct DiagonalDetector {
    config: ElliottConfig,
    flavor: DiagonalKind,
}

impl DiagonalDetector {
    pub fn new(config: ElliottConfig, flavor: DiagonalKind) -> ElliottResult<Self> {
        config.validate()?;
        Ok(Self { config, flavor })
    }
}

impl FormationDetector for DiagonalDetector {
    fn name(&self) -> &'static str {
        self.flavor.subkind_prefix()
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

        let mut results = Vec::new();
        for start in 0..=(pivots.len() - 6) {
            let tail = &pivots[start..start + 6];
            if !alternation_ok(tail) { continue; }
            let dir = Direction::from_first(tail[0].kind);
            let p = normalize(tail, dir);

            let w1 = p[1] - p[0];
            let w2 = p[1] - p[2];
            let w3 = p[3] - p[2];
            let w4 = p[3] - p[4];
            let w5 = p[5] - p[4];
            if w1 <= 0.0 || w2 <= 0.0 || w3 <= 0.0 || w4 <= 0.0 || w5 <= 0.0 { continue; }
            if p[2] <= p[0] { continue; }
            if w3 >= w1 || w5 >= w3 { continue; }

            let tight_w3 = nearest_fib_score(w3 / w1, &[0.5, 0.618, 0.786]);
            let tight_w5 = nearest_fib_score(w5 / w3, &[0.5, 0.618, 0.786]);
            let s_w2 = nearest_fib_score(w2 / w1, W2_REFS);
            let overlap_bonus = if p[4] <= p[1] { 1.0 } else { 0.85 };
            let score = mean_score(&[tight_w3, tight_w5, s_w2, overlap_bonus]);
            if (score as f32) < self.config.min_structural_score { continue; }

            // Determine leading vs ending by prior context:
            // - Leading: appears at START of a move (wave 1 or wave A).
            //   Prior move is OPPOSITE direction (price was falling before
            //   a bullish diagonal, or rising before a bearish one).
            // - Ending: appears at END of a move (wave 5 or wave C).
            //   Prior move is SAME direction (price was already rising
            //   before a bullish diagonal — it's the climax).
            let actual_flavor = {
                if start >= 2 {
                    // Look at the net move of ALL prior pivots (not just 3)
                    let prior_start_price = pivots[0].price.to_f64().unwrap_or(0.0);
                    let prior_end_price = pivots[start].price.to_f64().unwrap_or(0.0);
                    let prior_move = prior_end_price - prior_start_price;
                    let diag_move = p[5] - p[0]; // diagonal's net direction
                    // Same direction = ending (climax), opposite = leading (start)
                    if prior_move * diag_move > 0.0 {
                        DiagonalKind::Ending
                    } else {
                        DiagonalKind::Leading
                    }
                } else {
                    // No prior context — assume leading (start of data)
                    DiagonalKind::Leading
                }
            };

            let subkind = format!("{}_{}", actual_flavor.subkind_prefix(), dir.suffix());
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
