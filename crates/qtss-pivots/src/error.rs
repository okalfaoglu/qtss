use thiserror::Error;

#[derive(Debug, Error)]
pub enum PivotError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("non-monotonic bar timestamp at index {0}")]
    NonMonotonic(u64),
}

pub type PivotResult<T> = Result<T, PivotError>;
