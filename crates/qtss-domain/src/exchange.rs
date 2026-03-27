use serde::{Deserialize, Serialize};
use strum::EnumString;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum ExchangeId {
    Binance,
    #[strum(serialize = "custom")]
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketSegment {
    Spot,
    /// USDT-M / COIN-M ayrımı adapter katmanında.
    Futures,
    /// Mimari hazır; faz 2.
    Margin,
    Options,
}

/// Borsa adapter’ının sunduğu yetenekler (emir tipleri / veri).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeCapability {
    pub exchange: ExchangeId,
    pub segment: MarketSegment,
    pub supports_trailing_stop: bool,
    pub supports_post_only: bool,
    pub commission_from_api: bool,
}
