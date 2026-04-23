//! Shared types for the SMC detector catalog.

use crate::config::SmcConfig;
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::pivot::Pivot;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmcEventKind {
    Bos,
    Choch,
    Mss,
    LiquiditySweep,
    Fvi,
}

impl SmcEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bos => "bos",
            Self::Choch => "choch",
            Self::Mss => "mss",
            Self::LiquiditySweep => "liquidity_sweep",
            Self::Fvi => "fvi",
        }
    }
}

/// A single SMC detection. Unified across all 5 event families so the
/// engine writer can persist every kind through the same upsert path.
#[derive(Debug, Clone)]
pub struct SmcEvent {
    pub kind: SmcEventKind,
    pub variant: &'static str, // "bull" or "bear"
    pub score: f64,
    /// Where the event fired (the breaking-bar or sweep-bar index in
    /// the `bars` slice the evaluator received).
    pub bar_index: usize,
    /// Structural price — the swing level being broken / swept, or the
    /// middle candle's mid for FVI.
    pub reference_price: Decimal,
    /// Invalidation level — breaking this cancels the event. For BOS:
    /// re-crossing the broken swing. For sweeps: close back through
    /// the wick tip.
    pub invalidation_price: Decimal,
}

pub struct SmcSpec {
    pub name: &'static str,
    pub kind: SmcEventKind,
    /// Walk the bars + pivots and return every match the evaluator
    /// finds in the scan window. Returns a Vec so a single tick can
    /// publish multiple events of the same kind (rare but possible on
    /// high-activity days).
    pub eval: fn(&[Pivot], &[Bar], &SmcConfig) -> Vec<SmcEvent>,
}
