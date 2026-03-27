use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type LeaderId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopySubscription {
    pub id: Uuid,
    pub leader_user_id: Uuid,
    pub follower_user_id: Uuid,
    pub rule: CopyRule,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyRule {
    /// Leader pozisyonuna göre çarpan (örn. 0.5 = yarım boy).
    pub size_multiplier: Decimal,
    pub max_slippage_bps: i32,
    pub max_latency_ms: i64,
    pub min_notional: Option<Decimal>,
    pub max_notional: Option<Decimal>,
    /// Sadece long / sadece short gibi filtreler genişletilebilir.
    pub symbol_allowlist: Vec<String>,
}
