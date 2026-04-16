//! Error type.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VProfileError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("insufficient input: {0}")]
    InsufficientInput(String),
    #[error("numeric overflow / conversion failure: {0}")]
    Numeric(String),
}

pub type VProfileResult<T> = Result<T, VProfileError>;
