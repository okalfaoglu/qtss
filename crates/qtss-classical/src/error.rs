use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClassicalError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

pub type ClassicalResult<T> = Result<T, ClassicalError>;
