//! Placeholder live gateway for venues without an HTTP adapter yet (`QTSS_MASTER_DEV_GUIDE` §2.3.12).

use async_trait::async_trait;
use qtss_domain::exchange::ExchangeId;
use qtss_domain::orders::OrderIntent;
use qtss_domain::symbol::InstrumentId;
use rust_decimal::Decimal;
use tracing::instrument;
use uuid::Uuid;

use crate::gateway::{ExecutionError, ExecutionGateway};

/// Returns a clear [`ExecutionError::Exchange`] until Bybit / OKX (or other) adapters exist.
#[derive(Debug, Clone, Copy)]
pub struct UnsupportedLiveGateway {
    pub exchange: ExchangeId,
}

impl UnsupportedLiveGateway {
    #[must_use]
    pub const fn new(exchange: ExchangeId) -> Self {
        Self { exchange }
    }

    fn not_implemented(&self) -> ExecutionError {
        ExecutionError::Exchange(format!(
            "{}: live execution adapter not implemented (use Binance or dry mode)",
            self.exchange
        ))
    }
}

#[async_trait]
impl ExecutionGateway for UnsupportedLiveGateway {
    fn set_reference_price(
        &self,
        _instrument: &InstrumentId,
        _price: Decimal,
    ) -> Result<(), ExecutionError> {
        Ok(())
    }

    #[instrument(skip(self, _intent), fields(exchange = %self.exchange))]
    async fn place(&self, _intent: OrderIntent) -> Result<Uuid, ExecutionError> {
        Err(self.not_implemented())
    }

    async fn cancel(&self, _client_order_id: Uuid) -> Result<(), ExecutionError> {
        Err(self.not_implemented())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bybit_place_returns_error() {
        let gw = UnsupportedLiveGateway::new(ExchangeId::Bybit);
        let intent = OrderIntent {
            instrument: InstrumentId {
                exchange: ExchangeId::Bybit,
                segment: qtss_domain::exchange::MarketSegment::Futures,
                symbol: "BTCUSDT".into(),
            },
            side: qtss_domain::orders::OrderSide::Buy,
            quantity: rust_decimal::Decimal::ONE,
            order_type: qtss_domain::orders::OrderType::Market,
            time_in_force: qtss_domain::orders::TimeInForce::Gtc,
            requires_human_approval: false,
            futures: None,
        };
        let e = gw.place(intent).await.unwrap_err();
        let ExecutionError::Exchange(msg) = e else {
            panic!("expected Exchange variant");
        };
        assert!(msg.contains("bybit"), "{msg}");
        assert!(msg.contains("not implemented"), "{msg}");
    }

    #[tokio::test]
    async fn okx_place_returns_error() {
        let gw = UnsupportedLiveGateway::new(ExchangeId::Okx);
        let intent = OrderIntent {
            instrument: InstrumentId {
                exchange: ExchangeId::Okx,
                segment: qtss_domain::exchange::MarketSegment::Futures,
                symbol: "BTCUSDT".into(),
            },
            side: qtss_domain::orders::OrderSide::Buy,
            quantity: rust_decimal::Decimal::ONE,
            order_type: qtss_domain::orders::OrderType::Market,
            time_in_force: qtss_domain::orders::TimeInForce::Gtc,
            requires_human_approval: false,
            futures: None,
        };
        let e = gw.place(intent).await.unwrap_err();
        let ExecutionError::Exchange(msg) = e else {
            panic!("expected Exchange variant");
        };
        assert!(msg.contains("okx"), "{msg}");
        assert!(msg.contains("not implemented"), "{msg}");
    }
}
