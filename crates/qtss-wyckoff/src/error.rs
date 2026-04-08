use thiserror::Error;

#[derive(Debug, Error)]
pub enum WyckoffError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

pub type WyckoffResult<T> = Result<T, WyckoffError>;
