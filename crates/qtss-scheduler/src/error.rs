use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("unknown handler: {0}")]
    UnknownHandler(String),
    #[error("invalid schedule expression: {0}")]
    InvalidSchedule(String),
    #[error("handler failed: {0}")]
    HandlerFailed(String),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub type SchedulerResult<T> = Result<T, SchedulerError>;
