use serde::{Deserialize, Serialize};

use crate::exchange::{ExchangeId, MarketSegment};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstrumentId {
    pub exchange: ExchangeId,
    pub segment: MarketSegment,
    /// Borsa ham sembolü (örn. BTCUSDT).
    pub symbol: String,
}
