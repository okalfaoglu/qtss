use serde::{Deserialize, Serialize};

use crate::exchange::{ExchangeId, MarketSegment};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstrumentId {
    pub exchange: ExchangeId,
    pub segment: MarketSegment,
    /// Borsa ham sembolü (örn. BTCUSDT).
    pub symbol: String,
}

#[cfg(test)]
mod tests {
    use super::InstrumentId;
    use crate::exchange::{ExchangeId, MarketSegment};

    #[test]
    fn instrument_id_json_roundtrip() {
        let id = InstrumentId {
            exchange: ExchangeId::Binance,
            segment: MarketSegment::Futures,
            symbol: "BTCUSDT".to_string(),
        };
        let j = serde_json::to_string(&id).unwrap();
        let back: InstrumentId = serde_json::from_str(&j).unwrap();
        assert_eq!(back, id);
    }
}
