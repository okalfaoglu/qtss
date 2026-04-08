use thiserror::Error;

#[derive(Debug, Error)]
pub enum RiskError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

pub type RiskResult<T> = Result<T, RiskError>;
