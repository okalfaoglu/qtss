//! TBM runtime configuration. Every tunable lives here so the worker
//! can hydrate it from `system_config` (CLAUDE.md #2 — no hardcoded
//! constants in code paths). The struct is plain data; the loader
//! lives in `qtss-worker` next to the v2 detector binding so this
//! crate stays storage-agnostic.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TbmPillarWeights {
    pub momentum: f64,
    pub volume: f64,
    pub structure: f64,
    pub onchain: f64,
}

impl Default for TbmPillarWeights {
    fn default() -> Self {
        Self {
            momentum: 0.30,
            volume: 0.25,
            structure: 0.30,
            onchain: 0.15,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TbmSetupTuning {
    /// Minimum aggregate score required to emit a setup.
    pub min_score: f64,
    /// Minimum number of pillars whose individual score crosses
    /// `pillar_active_threshold` for the setup to count.
    pub min_active_pillars: usize,
    /// Per-pillar score threshold above which a pillar counts as
    /// "active" for the active-pillars rule.
    pub pillar_active_threshold: f64,
}

impl Default for TbmSetupTuning {
    fn default() -> Self {
        Self {
            min_score: 50.0,
            min_active_pillars: 2,
            pillar_active_threshold: 20.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TbmMtfTuning {
    /// Minimum count of confirming sibling timeframes required before
    /// a setup is promoted to "confirmed".
    pub required_confirms: usize,
    /// Minimum alignment score (0–1) across the confirming TFs.
    pub min_alignment: f64,
}

impl Default for TbmMtfTuning {
    fn default() -> Self {
        Self {
            required_confirms: 2,
            min_alignment: 0.5,
        }
    }
}

/// Top-level TBM runtime config. The worker hydrates this from
/// `system_config` once per tick interval; the detector treats it as
/// immutable for the duration of the tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TbmConfig {
    pub enabled: bool,
    pub tick_interval_s: u64,
    pub lookback_bars: usize,
    pub weights: TbmPillarWeights,
    pub setup: TbmSetupTuning,
    pub mtf: TbmMtfTuning,
    pub onchain_enabled: bool,
}

impl Default for TbmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tick_interval_s: 60,
            lookback_bars: 300,
            weights: TbmPillarWeights::default(),
            setup: TbmSetupTuning::default(),
            mtf: TbmMtfTuning::default(),
            onchain_enabled: false,
        }
    }
}
