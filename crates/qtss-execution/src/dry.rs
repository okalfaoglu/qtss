use async_trait::async_trait;
use tracing::instrument;
use qtss_common::{log_business, Loggable, QtssLogLevel};
use qtss_domain::orders::OrderIntent;
use uuid::Uuid;

use crate::gateway::{ExecutionError, ExecutionGateway};

pub struct DryRunGateway;

impl Loggable for DryRunGateway {
    const MODULE: &'static str = "qtss_execution::dry";
}

#[async_trait]
impl ExecutionGateway for DryRunGateway {
    #[instrument(skip(self, intent))]
    async fn place(&self, intent: OrderIntent) -> Result<Uuid, ExecutionError> {
        let id = Uuid::new_v4();
        log_business(
            QtssLogLevel::Info,
            Self::MODULE,
            format!("dry place {:?} qty {}", intent.side, intent.quantity),
        );
        Ok(id)
    }

    async fn cancel(&self, _client_order_id: Uuid) -> Result<(), ExecutionError> {
        log_business(QtssLogLevel::Debug, Self::MODULE, "dry cancel");
        Ok(())
    }
}
