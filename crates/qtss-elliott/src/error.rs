use thiserror::Error;

#[derive(Debug, Error)]
pub enum ElliottError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

pub type ElliottResult<T> = Result<T, ElliottError>;
