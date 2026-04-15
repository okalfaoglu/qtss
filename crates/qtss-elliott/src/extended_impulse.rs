//! Extended-impulse detector.
//!
//! Per Frost & Prechter, exactly one of waves 1, 3, or 5 in any impulse
//! is "extended" — markedly longer than the other two. The standard
//! threshold is that the extended wave is at least 1.618× the longer
//! of the other two impulse waves. Identifying *which* wave is extended
//! changes how the move should be projected and where the structure
//! invalidates.
//!
//! This detector reuses the same 6-pivot tail as `ImpulseDetector` but
//! does not re-run the impulse rules — it assumes the orchestrator has
//! already paired this detector with the impulse one and only emits
//! when the extension test passes. The point is to surface the *flavor*
//! (`impulse_w1_extended_<dir>` / `_w3_extended_` / `_w5_extended_`) so
//! downstream consumers can react accordingly.

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

const ANCHOR_LABELS: &[&str] = &["0", "1", "2", "3", "4", "5"];
/// The other two waves must be no more than this fraction of the
/// extended one for the structure to qualify as "extended".
const EXTENSION_RATIO: f64 = 1.0 / 1.618;

pub struct ExtendedImpulseDetector {
    config: ElliottConfig,
}

impl ExtendedImpulseDetector {
    pub fn new(config: ElliottConfig) -> ElliottResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }
}

impl FormationDetector for ExtendedImpulseDetector {
    fn name(&self) -> &'static str {
        "extended_impulse"
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
            let w3 = p[3] - p[2];
            let w5 = p[5] - p[4];
            if w1 <= 0.0 || w3 <= 0.0 || w5 <= 0.0 { continue; }
            if p[2] <= p[0] || p[4] <= p[1] { continue; }
            if w3 < w1 && w3 < w5 { continue; }

            let waves = [("w1", w1), ("w3", w3), ("w5", w5)];
            let (label, longest) = waves
                .iter()
                .copied()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap();
            let others_max = waves
                .iter()
                .filter(|(l, _)| *l != label)
                .map(|(_, v)| *v)
                .fold(0.0_f64, f64::max);
            if others_max == 0.0 || others_max / longest > EXTENSION_RATIO { continue; }

            let ratio = longest / others_max;
            let s_ext = nearest_fib_score(ratio, &[1.618, 2.618, 4.236]);
            let s_w2 = nearest_fib_score((p[1] - p[2]) / w1, &[0.382, 0.5, 0.618]);
            let score = mean_score(&[s_ext, s_w2]);
            if (score as f32) < self.config.min_structural_score { continue; }

            let subkind = format!("impulse_{label}_extended_{}", dir.suffix());
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
