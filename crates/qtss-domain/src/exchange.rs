use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Supported or planned venue identifiers (`QTSS_MASTER_DEV_GUIDE` §2.3.12).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumString, Display,
)]
#[strum(serialize_all = "snake_case")]
pub enum ExchangeId {
    Binance,
    /// Perpetual / futures; live HTTP adapter not implemented (`qtss-execution` stub gateway).
    Bybit,
    /// Spot + derivatives umbrella id; live adapter not implemented.
    Okx,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn exchange_id_from_snake_case_str() {
        assert_eq!(ExchangeId::from_str("binance").unwrap(), ExchangeId::Binance);
        assert_eq!(ExchangeId::from_str("bybit").unwrap(), ExchangeId::Bybit);
        assert_eq!(ExchangeId::from_str("okx").unwrap(), ExchangeId::Okx);
        assert_eq!(ExchangeId::from_str("custom").unwrap(), ExchangeId::Custom);
    }

    #[test]
    fn exchange_id_display_snake_case() {
        assert_eq!(ExchangeId::Binance.to_string(), "binance");
        assert_eq!(ExchangeId::Bybit.to_string(), "bybit");
        assert_eq!(ExchangeId::Okx.to_string(), "okx");
    }

    #[test]
    fn exchange_id_json_roundtrip_new_variants() {
        for ex in [ExchangeId::Bybit, ExchangeId::Okx] {
            let j = serde_json::to_string(&ex).unwrap();
            let back: ExchangeId = serde_json::from_str(&j).unwrap();
            assert_eq!(back, ex);
        }
    }

    #[test]
    fn market_segment_serializes_snake_case() {
        let j = serde_json::to_string(&MarketSegment::Futures).unwrap();
        assert_eq!(j, "\"futures\"");
    }
}
