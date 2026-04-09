use serde::{Deserialize, Serialize};

/// Direction the layer is voting for. Mirrors the Onchain crate enum
/// shape so the worker can map both into a single space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfluenceDirection {
    Long,
    Short,
    Neutral,
}

impl ConfluenceDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            ConfluenceDirection::Long => "long",
            ConfluenceDirection::Short => "short",
            ConfluenceDirection::Neutral => "neutral",
        }
    }

    pub fn sign(self) -> f64 {
        match self {
            ConfluenceDirection::Long => 1.0,
            ConfluenceDirection::Short => -1.0,
            ConfluenceDirection::Neutral => 0.0,
        }
    }
}

/// One detector vote contributing to the confluence layer count.
#[derive(Debug, Clone)]
pub struct DetectionVote {
    pub family: String, // "elliott" / "harmonic" / "classical" / "wyckoff" / "range"
    pub subkind: String,
    pub direction: ConfluenceDirection,
    pub structural_score: f32, // 0..1
}

/// Everything the scorer needs for one (symbol, timeframe). The worker
/// loop builds this from `qtss_v2_tbm_metrics`, `qtss_v2_detections`
/// and `qtss_v2_onchain_metrics`.
#[derive(Debug, Clone, Default)]
pub struct ConfluenceInputs {
    /// TBM aggregate score in `[-1, +1]`. None when no fresh row.
    pub tbm_score: Option<f64>,
    /// TBM confidence in `[0, 1]`. Used to weigh the TBM layer.
    pub tbm_confidence: Option<f64>,
    /// Detector votes from `qtss_v2_detections`. One row per (family,
    /// subkind). Same family voting twice counts twice (e.g. two
    /// Elliott formations both LONG).
    pub detections: Vec<DetectionVote>,
    /// Aggregate Onchain score in `[-1, +1]` from
    /// `qtss_v2_onchain_metrics.aggregate_score`.
    pub onchain: Option<f64>,
}

/// All weights live in `system_config` (CLAUDE.md #2). Loaded by the
/// worker; this struct is just the carrier.
#[derive(Debug, Clone, Copy)]
pub struct ConfluenceWeights {
    pub elliott: f64,
    pub harmonic: f64,
    pub classical: f64,
    pub wyckoff: f64,
    pub range: f64,
    pub tbm: f64,
    pub onchain: f64,
    /// Minimum number of distinct voting layers required for `guven`
    /// to be non-zero. Default seeded by migration 0031 = 3.
    pub min_layers: u32,
}

impl Default for ConfluenceWeights {
    fn default() -> Self {
        Self {
            elliott: 0.30,
            harmonic: 0.20,
            classical: 0.15,
            wyckoff: 0.15,
            range: 0.10,
            tbm: 0.10,
            onchain: 0.10,
            min_layers: 3,
        }
    }
}

/// Output of [`crate::score_confluence`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfluenceReading {
    /// `[-1, +1]` — TBM raw passed through (or 0 when TBM missing).
    pub erken_uyari: f64,
    /// `[0, 1]` — weighted agreement strength. Hard 0 when
    /// `layer_count < min_layers`.
    pub guven: f64,
    pub direction: ConfluenceDirection,
    /// How many distinct layers had an opinion (TBM, Onchain, and each
    /// detection vote each count as one).
    pub layer_count: u32,
    /// Human-readable per-layer breakdown for logs and the GUI tooltip.
    pub details: Vec<String>,
}
