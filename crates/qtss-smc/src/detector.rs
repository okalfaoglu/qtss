//! SmcDetector — iterates every [`SmcSpec`] through the same loop and
//! returns all matches above `min_structural_score`.

use crate::config::SmcConfig;
use crate::event::{SmcEvent, SmcEventKind, SmcSpec};
use crate::events::{eval_bos, eval_choch, eval_fvi, eval_liquidity_sweep, eval_mss};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::pivot::PivotTree;

pub static SMC_SPECS: &[SmcSpec] = &[
    SmcSpec {
        name: "bos",
        kind: SmcEventKind::Bos,
        eval: eval_bos,
    },
    SmcSpec {
        name: "choch",
        kind: SmcEventKind::Choch,
        eval: eval_choch,
    },
    SmcSpec {
        name: "mss",
        kind: SmcEventKind::Mss,
        eval: eval_mss,
    },
    SmcSpec {
        name: "liquidity_sweep",
        kind: SmcEventKind::LiquiditySweep,
        eval: eval_liquidity_sweep,
    },
    SmcSpec {
        name: "fvi",
        kind: SmcEventKind::Fvi,
        eval: eval_fvi,
    },
];

pub struct SmcDetector {
    cfg: SmcConfig,
}

impl SmcDetector {
    pub fn new(cfg: SmcConfig) -> Result<Self, String> {
        cfg.validate()?;
        Ok(Self { cfg })
    }

    pub fn config(&self) -> &SmcConfig {
        &self.cfg
    }

    /// Run every spec and collect events above the threshold. The
    /// engine writer flattens the returned Vec into detection rows.
    pub fn detect(&self, tree: &PivotTree, bars: &[Bar]) -> Vec<SmcEvent> {
        let pivots = tree.at_level(self.cfg.pivot_level);
        let mut out = Vec::new();
        for spec in SMC_SPECS {
            for ev in (spec.eval)(pivots, bars, &self.cfg) {
                if (ev.score as f32) >= self.cfg.min_structural_score {
                    out.push(ev);
                }
            }
        }
        out
    }
}
