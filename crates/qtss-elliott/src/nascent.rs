//! Nascent impulse detector — Faz 15 / LuxAlgo madde #4.
//!
//! Fires **before** wave 5 completes so the setup engine can size a
//! position on the 3rd-wave extension (historically the most tradeable
//! Elliott leg). Unlike [`ImpulseDetector`], which needs 6 pivots, this
//! one inspects the most recent 4 pivots (`0-1-2-3`) and emits a
//! `PatternState::Forming` detection with subkind `impulse_nascent_*`.
//!
//! Validity rules (normalized to bullish-positive frame):
//!   1. Strict alternation of pivot kinds — `p0 < p1 > p2 < p3`.
//!   2. Wave 2 retrace ∈ (0.236, 0.786) of wave 1 — anything outside
//!      that range is more likely the start of a corrective flat / ZZ.
//!   3. Wave 3 has already exceeded wave 1 extreme (`p3 > p1`) — without
//!      this the pattern is still ambiguous (could be an ABC top).
//!   4. (Optional) Wave 3 length ≥ wave 1 length — the canonical
//!      "wave-3-not-shortest" rule evaluated with what we have so far.
//!
//! Invalidation price = p0 (same as full impulse). Anchors emit labels
//! `0,1,2,3` (four points). Structural score = wave-2-retrace fib
//! proximity × wave-3-extension fib proximity.
//!
//! The detector is stateless; deduplication + incremental updates are
//! the orchestrator's responsibility (Faz 14.A9).

use crate::common::{alternation_ok, label_anchors, mean_score, nearest_fib_score, normalize, Direction};
use crate::config::ElliottConfig;
use crate::error::ElliottResult;
use crate::formation::FormationDetector;
use qtss_domain::v2::detection::{Detection, PatternKind, PatternState};
use qtss_domain::v2::instrument::Instrument;
use qtss_domain::v2::pivot::PivotTree;
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;

const WAVE2_REFS: &[f64] = &[0.382, 0.5, 0.618];
const WAVE3_EXT_REFS: &[f64] = &[1.618, 2.0, 2.618];
const ANCHOR_LABELS: &[&str] = &["0", "1", "2", "3"];

pub struct NascentImpulseDetector {
    config: ElliottConfig,
}

impl NascentImpulseDetector {
    pub fn new(config: ElliottConfig) -> ElliottResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }
}

impl FormationDetector for NascentImpulseDetector {
    fn name(&self) -> &'static str {
        "impulse_nascent"
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
            let dir = Direction::from_first(tail[0].kind);
            let p = normalize(tail, dir);

            // Rule 1: alternation in normalized frame (guaranteed by
            // alternation_ok + direction selection, but we still need
            // strict inequalities — flat pivots are skipped).
            if !(p[0] < p[1] && p[1] > p[2] && p[2] < p[3]) {
                continue;
            }

            let w1 = p[1] - p[0];
            let w2 = p[1] - p[2];
            let w3_so_far = p[3] - p[2];
            if w1 <= 0.0 || w2 <= 0.0 || w3_so_far <= 0.0 {
                continue;
            }

            // Rule 2: w2 retrace in (0.236, 0.786).
            let w2_ret = w2 / w1;
            if !(0.236..=0.786).contains(&w2_ret) {
                continue;
            }

            // Rule 3: w3 already broke above p1 (bullish frame).
            if p[3] <= p[1] {
                continue;
            }

            // Rule 4: w3 not shorter than w1 — aligns with the canonical
            // not-shortest rule evaluated with what's available.
            if w3_so_far < w1 * 0.9 {
                continue;
            }

            let w3_ext = w3_so_far / w1;
            let s2 = nearest_fib_score(w2_ret, WAVE2_REFS);
            let s3 = nearest_fib_score(w3_ext, WAVE3_EXT_REFS);
            let score = mean_score(&[s2, s3]);

            if (score as f32) < self.config.min_structural_score {
                continue;
            }

            let subkind = format!("impulse_nascent_{}", dir.suffix());
            let anchors = label_anchors(tail, self.config.pivot_level, ANCHOR_LABELS);
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
            ));
        }

        results
    }
}
