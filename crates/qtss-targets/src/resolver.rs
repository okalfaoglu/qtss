//! Shared trait + data shapes.

use crate::config::TargetConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Direction of the intended trade. Used to sign-orient TP/SL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeDirection {
    Long,
    Short,
}

impl TradeDirection {
    pub fn from_i16(dir: i16) -> Option<Self> {
        match dir.signum() {
            1 => Some(Self::Long),
            -1 => Some(Self::Short),
            _ => None,
        }
    }
    pub fn sign(self) -> f64 {
        match self {
            Self::Long => 1.0,
            Self::Short => -1.0,
        }
    }
}

/// Which resolver produced the level (for audit + GUI coloring).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetSource {
    HarmonicPrz,
    VProfileMagnet,
    FibExtension,
    Structural,
    AtrBand,
}

impl TargetSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HarmonicPrz => "harmonic_prz",
            Self::VProfileMagnet => "vprofile_magnet",
            Self::FibExtension => "fib_extension",
            Self::Structural => "structural",
            Self::AtrBand => "atr_band",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetLevel {
    /// 1-based ordinal (TP1, TP2, TP3).
    pub ordinal: u8,
    pub price: f64,
    pub source: TargetSource,
    /// Reach probability hint 0..1 — derived from fib ratio / distance
    /// / historical hit-rate. Caller may ignore.
    pub hit_prob_hint: f64,
    /// Human-readable label for the chart/Telegram card.
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetSet {
    pub direction: TradeDirection,
    pub entry: f64,
    pub take_profits: Vec<TargetLevel>,
    /// Hard stop — closing beyond this cancels the position.
    pub stop_loss: f64,
    /// Soft invalidation — pattern geometry broken, consider exit.
    pub invalidation: f64,
    /// Source tag for the primary target (resolver that produced TP1).
    pub primary_source: TargetSource,
    pub notes: Vec<String>,
}

/// Context passed to every resolver. Each resolver picks what it needs;
/// the engine assembles the union of whatever's available.
#[derive(Debug, Clone)]
pub struct DetectionContext {
    /// Family of the detection ("harmonic", "classical", ...).
    pub family: String,
    /// Subkind for family-specific branching (e.g. "gartley_bull").
    pub subkind: String,
    pub direction: TradeDirection,
    pub entry: f64,
    /// Anchor points (label → price). Harmonic carries X/A/B/C/D;
    /// classical carries shoulder/neckline/etc.; range carries
    /// zone_high / zone_low.
    pub anchors: HashMap<String, f64>,
    /// Recent bar ATR — mandatory for AtrBand, optional elsewhere.
    pub atr: Option<f64>,
    /// Volume profile levels (VPOC/VAH/VAL). Keys map to lib conv.
    pub vprofile_levels: HashMap<String, f64>,
    /// Recent same-direction pivots (prices), newest first. Used by
    /// Structural.
    pub forward_pivots: Vec<f64>,
    /// Recent opposite-direction pivot (price). Used as SL base by
    /// Structural.
    pub opposite_pivot: Option<f64>,
}

/// A target resolver. Impls live in `resolvers/*.rs` — each returns
/// `None` when the context lacks the inputs it needs, so the registry
/// can fall through to the next resolver.
pub trait TargetResolver: Send + Sync {
    fn source(&self) -> TargetSource;
    fn resolve(&self, ctx: &DetectionContext, cfg: &TargetConfig) -> Option<TargetSet>;
}

/// Dispatch registry. Constructed once, queried per detection. Tries
/// resolvers in the configured order and returns the first successful
/// match — the order encodes preference (family-specific resolvers
/// first, ATR fallback last).
pub struct ResolverRegistry {
    pub resolvers: Vec<Arc<dyn TargetResolver>>,
    pub cfg: TargetConfig,
}

impl ResolverRegistry {
    pub fn new(resolvers: Vec<Arc<dyn TargetResolver>>, cfg: TargetConfig) -> Self {
        Self { resolvers, cfg }
    }

    pub fn resolve(&self, ctx: &DetectionContext) -> Option<TargetSet> {
        for r in &self.resolvers {
            if let Some(ts) = r.resolve(ctx, &self.cfg) {
                return Some(ts);
            }
        }
        None
    }
}
