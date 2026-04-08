use thiserror::Error;

#[derive(Debug, Error)]
pub enum TargetEngineError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

pub type TargetEngineResult<T> = Result<T, TargetEngineError>;
