use async_trait::async_trait;
use qtss_domain::orders::OrderIntent;
use qtss_domain::symbol::InstrumentId;
use rust_decimal::Decimal;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("borsa reddi: {0}")]
    Exchange(String),
    #[error("dry run bakiye yetersiz")]
    InsufficientPaper,
    #[error("onay bekleniyor")]
    PendingApproval,
    #[error("bilinmeyen: {0}")]
    Other(String),
}

#[derive(Debug, Clone)]
pub struct FillEvent {
    pub client_order_id: Uuid,
    pub avg_price: Decimal,
    pub quantity: Decimal,
    pub fee: Decimal,
}

#[async_trait]
pub trait ExecutionGateway: Send + Sync {
    /// Paper/dry: referans fiyat (market dolumu); canlı ağ geçitleri genelde no-op.
    fn set_reference_price(&self, instrument: &InstrumentId, price: Decimal) -> Result<(), ExecutionError>;

    async fn place(&self, intent: OrderIntent) -> Result<Uuid, ExecutionError>;
    async fn cancel(&self, client_order_id: Uuid) -> Result<(), ExecutionError>;
}
