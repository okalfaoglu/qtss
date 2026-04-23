use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderFlowEventKind {
    LiquidationCluster,
    BlockTrade,
    CvdDivergence,
}

impl OrderFlowEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LiquidationCluster => "liquidation_cluster",
            Self::BlockTrade => "block_trade",
            Self::CvdDivergence => "cvd_divergence",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OrderFlowEvent {
    pub kind: OrderFlowEventKind,
    /// "bull" / "bear" / "neutral"
    pub variant: &'static str,
    pub score: f64,
    /// Notional USD magnitude or divergence size — interpretation
    /// depends on `kind`.
    pub magnitude: f64,
    /// Reference price (liquidation avg price or current bar close).
    pub reference_price: f64,
    /// ms-epoch of the event bar.
    pub event_time_ms: i64,
    pub note: String,
}
