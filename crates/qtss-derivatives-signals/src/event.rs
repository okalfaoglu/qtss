//! Shared detection event shape.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivEventKind {
    FundingSpike,
    OiImbalance,
    BasisDislocation,
    LongShortExtreme,
    TakerFlowImbalance,
}

impl DerivEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FundingSpike => "funding_spike",
            Self::OiImbalance => "oi_imbalance",
            Self::BasisDislocation => "basis_dislocation",
            Self::LongShortExtreme => "long_short_extreme",
            Self::TakerFlowImbalance => "taker_flow_imbalance",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DerivEvent {
    pub kind: DerivEventKind,
    /// "bull" / "bear" / "neutral".
    pub variant: &'static str,
    /// 0..1 — higher = more extreme reading.
    pub score: f64,
    /// Current value of the underlying metric (funding rate, OI delta,
    /// basis pct, LSR, taker ratio). Used for audit and the chart label.
    pub metric_value: f64,
    /// Baseline against which `metric_value` is judged (rolling mean
    /// / threshold / prior reading).
    pub baseline_value: f64,
    /// Human-readable note (stored in raw_meta).
    pub note: String,
}
