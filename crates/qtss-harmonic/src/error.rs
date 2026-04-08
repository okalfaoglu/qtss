use thiserror::Error;

#[derive(Debug, Error)]
pub enum HarmonicError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

pub type HarmonicResult<T> = Result<T, HarmonicError>;
