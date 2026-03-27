use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::exchange::ExchangeId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommissionQuote {
    pub maker_rate: Decimal,
    pub taker_rate: Decimal,
    pub source: CommissionSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommissionSource {
    ExchangeApi { exchange: ExchangeId },
    ConfigFallback { key: String },
}

pub trait CommissionResolver: Send + Sync {
    fn resolve(&self, exchange: ExchangeId, symbol: &str) -> CommissionQuote;
}
