use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("invalid intent: {0}")]
    InvalidIntent(String),
    #[error("adapter error: {0}")]
    Adapter(String),
    #[error("no adapter registered for mode {0:?}")]
    NoAdapter(qtss_domain::execution::ExecutionMode),
    #[error("order not found: {0}")]
    OrderNotFound(uuid::Uuid),
}

pub type ExecutionResult<T> = Result<T, ExecutionError>;
