//! Truncated-fifth detector.
//!
//! A truncated 5th occurs when wave 5 fails to exceed the high (or low,
//! for bearish) of wave 3. The structure is otherwise a valid impulse —
//! all rules pass — but the final wave runs out of momentum. It almost
//! always signals strong opposing pressure and an imminent reversal.
//!
//! Pivot signature: 6 alternating pivots, but `p5 < p3` in the
//! normalized bullish frame. We can't reuse `alternation_ok` strictly
//! here because the truncated tail breaks the high/low/high/low/high/
//! low cadence at the very last pivot — instead we accept the first 5
//! alternating and then *reject* the strict ordering at p5.
//!
//! Because the standard impulse detector also looks at 6 pivots, the
//! truncated case will *not* fire from `ImpulseDetector` (it requires
//! `p5 > p3`). This detector is a parallel scanner that looks for the
//! same 6-pivot tail with the relaxed last-leg rule.

use crate::common::{label_anchors, mean_score, nearest_fib_score, normalize, Direction};
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

const ANCHOR_LABELS: &[&str] = &["0", "1", "2", "3", "4", "5"];

pub struct TruncatedFifthDetector {
    config: ElliottConfig,
}

impl TruncatedFifthDetector {
    pub fn new(config: ElliottConfig) -> ElliottResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }
}

impl FormationDetector for TruncatedFifthDetector {
    fn name(&self) -> &'static str {
        "truncated_fifth"
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
        // Strict alternation for the first 5 pivots; the 6th is
        // intentionally allowed to be the *same* kind-direction as the
        // 5th's predecessor (truncation).
        let head_alt = tail
            .windows(2)
            .take(4)
            .all(|w| w[0].kind != w[1].kind);
        if !head_alt {
            return Vec::new();
        }
        let dir = Direction::from_first(tail[0].kind);
        let p = normalize(tail, dir);

        let w1 = p[1] - p[0];
        let w3 = p[3] - p[2];
        let w5 = p[5] - p[4];
        if w1 <= 0.0 || w3 <= 0.0 || w5 <= 0.0 {
            return Vec::new();
        }
        // All standard impulse rules except the implicit "p5 > p3":
        if p[2] <= p[0] {
            return Vec::new();
        }
        if w3 < w1 && w3 < w5 {
            return Vec::new();
        }
        if p[4] <= p[1] {
            return Vec::new();
        }
        // The diagnostic: p5 *fails* to exceed p3.
        if p[5] >= p[3] {
            return Vec::new();
        }

        // Score: standard impulse fib proximities, with a penalty for
        // how far short of p3 the p5 falls (deeper truncation = stronger
        // signal but also less "textbook impulse").
        let s2 = nearest_fib_score((p[1] - p[2]) / w1, &[0.382, 0.5, 0.618]);
        let s3 = nearest_fib_score(w3 / w1, &[1.618, 2.0, 2.618]);
        let s4 = nearest_fib_score((p[3] - p[4]) / w3, &[0.236, 0.382, 0.5]);
        let truncation_depth = (p[3] - p[5]) / w3;
        let s_trunc = nearest_fib_score(truncation_depth, &[0.05, 0.1, 0.2]);
        let score = mean_score(&[s2, s3, s4, s_trunc]);
        if (score as f32) < self.config.min_structural_score {
            return Vec::new();
        }

        let subkind = format!("impulse_truncated_5_{}", dir.suffix());
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
