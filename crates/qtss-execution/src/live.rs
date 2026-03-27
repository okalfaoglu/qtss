use async_trait::async_trait;
use qtss_common::{log_business, Loggable, QtssLogLevel};
use qtss_domain::orders::OrderIntent;
use tracing::instrument;
use uuid::Uuid;

use crate::gateway::{ExecutionError, ExecutionGateway};

/// Genel canlı gateway yedek iskeleti. Binance için [`crate::BinanceLiveGateway`] kullanın.
pub struct LiveGateway;

impl Loggable for LiveGateway {
    const MODULE: &'static str = "qtss_execution::live";
}

#[async_trait]
impl ExecutionGateway for LiveGateway {
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
