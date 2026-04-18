use thiserror::Error;

#[derive(Debug, Error)]
pub enum CandleError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

pub type CandleResult<T> = Result<T, CandleError>;
