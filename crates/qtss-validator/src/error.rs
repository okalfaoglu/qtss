use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidatorError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

pub type ValidatorResult<T> = Result<T, ValidatorError>;
