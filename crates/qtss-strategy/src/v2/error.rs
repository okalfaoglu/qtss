use thiserror::Error;

#[derive(Debug, Error)]
pub enum StrategyError {
    #[error("strategy '{0}' rejected the signal: {1}")]
    Rejected(String, String),
    #[error("invalid strategy config: {0}")]
    InvalidConfig(String),
    #[error("internal: {0}")]
    Internal(String),
}

pub type StrategyResult<T> = Result<T, StrategyError>;
