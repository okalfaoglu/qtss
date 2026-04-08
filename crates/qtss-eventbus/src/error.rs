use thiserror::Error;

pub type EventBusResult<T> = Result<T, EventBusError>;

#[derive(Debug, Error)]
pub enum EventBusError {
    #[error("topic '{0}' has no active subscribers")]
    NoSubscribers(String),

    #[error("subscriber lagged on topic '{topic}' (skipped {skipped} messages)")]
    Lagged { topic: String, skipped: u64 },

    #[error("channel closed for topic '{0}'")]
    Closed(String),

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}
