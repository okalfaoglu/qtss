use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegimeError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("non-monotonic bar timestamp at index {0}")]
    NonMonotonic(u64),
}

pub type RegimeResult<T> = Result<T, RegimeError>;
