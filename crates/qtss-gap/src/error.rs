use thiserror::Error;

#[derive(Debug, Error)]
pub enum GapError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

pub type GapResult<T> = Result<T, GapError>;
