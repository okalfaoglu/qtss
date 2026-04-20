//! Forming impulse detector — Faz 14.A13.
//!
//! Bridges the gap between [`NascentImpulseDetector`] (4 pivots) and
//! [`ImpulseDetector`] (6 pivots). Operates on 5-pivot windows
//! `0-1-2-3-4` and emits `impulse_forming_*` while wave 5 is still in
//! progress. Once wave 5 completes and the 6-pivot window validates,
//! the full impulse takes over (identity tuple differs because of the
//! distinct subkind, so no upsert conflict).
//!
//! Rules (normalized bullish-positive frame):
//!   1. Strict alternation `p0<p1>p2<p3>p4`.
//!   2. Wave 2 does not break below p0 (with the same tolerance band
//!      the 6-pivot rule now uses).
//!   3. Wave 3 is not the shortest (comparing w1 vs. w3 only — wave 5
//!      is still forming so the three-way check waits).
//!   4. Wave 4 does not enter wave 1 territory (`p4 > p1`).
//!
//! Structural score = fib proximity of w2-retrace × w3-extension ×
//! w4-retrace (3-part mean; same basket the full detector uses minus
//! the wave-5 term).

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
const WAVE4_REFS: &[f64] = &[0.236, 0.382, 0.5];
const ANCHOR_LABELS: &[&str] = &["0", "1", "2", "3", "4"];

pub struct FormingImpulseDetector {
    config: ElliottConfig,
}

impl FormingImpulseDetector {
    pub fn new(config: ElliottConfig) -> ElliottResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }
}

impl FormationDetector for FormingImpulseDetector {
    fn name(&self) -> &'static str {
        "impulse_forming"
    }

    fn detect(
        &self,
        tree: &PivotTree,
        instrument: &Instrument,
        timeframe: Timeframe,
        regime: &RegimeSnapshot,
    ) -> Vec<Detection> {
        let pivots = tree.at_level(self.config.pivot_level);
        if pivots.len() < 5 {
            return Vec::new();
        }
        let mut results = Vec::new();

        for start in 0..=(pivots.len() - 5) {
            let tail = &pivots[start..start + 5];
            if !alternation_ok(tail) {
                continue;
            }
            let dir = Direction::from_first(tail[0].kind);
            let p = normalize(tail, dir);

            if !(p[0] < p[1] && p[1] > p[2] && p[2] < p[3] && p[3] > p[4]) {
                continue;
            }

            let w1 = p[1] - p[0];
            let w3 = p[3] - p[2];
            let w2 = p[1] - p[2];
            let w4 = p[3] - p[4];
            if w1 <= 0.0 || w3 <= 0.0 || w2 <= 0.0 || w4 <= 0.0 {
                continue;
            }

            // Rule 2: wave 2 does not break past p0 (with tolerance).
            let tol = p[0].abs() * 1e-3;
            if p[2] < p[0] - tol {
                continue;
            }
            // Rule 3: wave 3 ≥ wave 1 (the partial "not shortest" check).
            if w3 < w1 * 0.9 {
                continue;
            }
            // Rule 4: wave 4 ∈ (0, w3); no overlap with wave 1.
            if p[4] <= p[1] {
                continue;
            }

            let w2_ret = w2 / w1;
            let w3_ext = w3 / w1;
            let w4_ret = w4 / w3;
            let score = mean_score(&[
                nearest_fib_score(w2_ret, WAVE2_REFS),
                nearest_fib_score(w3_ext, WAVE3_EXT_REFS),
                nearest_fib_score(w4_ret, WAVE4_REFS),
            ]);
            if (score as f32) < self.config.min_structural_score {
                continue;
            }

            let subkind = format!("impulse_forming_{}", dir.suffix());
            let anchors = label_anchors(tail, self.config.pivot_level, ANCHOR_LABELS);
            results.push(Detection::new(
                instrument.clone(),
                timeframe,
                PatternKind::Elliott(subkind),
                PatternState::Forming,
                anchors,
                score as f32,
                tail[0].price,
                regime.clone(),
            ));
        }

        results
    }
}
