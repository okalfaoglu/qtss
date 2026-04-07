//! ## Uyarı
//! [`LiveGateway`] **kasıtlı iskelet**tir: emir göndermez, yalnızca uyarı loglar ve hata döner.
//! Üretim yönlendirmesinde [`crate::BinanceLiveGateway`] veya venue-özel bir [`ExecutionGateway`] kullanın.
//! Bu tip yanlışlıkla DI ile canlı yola bağlanırsa tüm `place` çağrıları başarısız olur (sessizce
//! kâr sanılmamalı — log ve `ExecutionError` üretir).

use async_trait::async_trait;
use qtss_common::{log_business, Loggable, QtssLogLevel};
use qtss_domain::orders::OrderIntent;
use qtss_domain::symbol::InstrumentId;
use rust_decimal::Decimal;
use tracing::instrument;
use uuid::Uuid;

use crate::gateway::{ExecutionError, ExecutionGateway};

/// Genel canlı gateway **iskeleti** (bağlı değil). Binance: [`crate::BinanceLiveGateway`]. Diğer borsalar: [`crate::UnsupportedLiveGateway`].
pub struct LiveGateway;

impl Loggable for LiveGateway {
    const MODULE: &'static str = "qtss_execution::live";
}

#[async_trait]
impl ExecutionGateway for LiveGateway {
    fn set_reference_price(
        &self,
        _instrument: &InstrumentId,
        _price: Decimal,
    ) -> Result<(), ExecutionError> {
        Ok(())
    }

    #[instrument(skip(self, _intent))]
    async fn place(&self, _intent: OrderIntent) -> Result<Uuid, ExecutionError> {
        log_business(
            QtssLogLevel::Warning,
            Self::MODULE,
            "LiveGateway henüz bağlı değil — adapter eklenince doldurulacak",
        );
        Err(ExecutionError::Exchange(
            "live adapter not wired".into(),
        ))
    }

    async fn cancel(&self, _client_order_id: Uuid) -> Result<(), ExecutionError> {
        Err(ExecutionError::Exchange(
            "live adapter not wired".into(),
        ))
    }
}
