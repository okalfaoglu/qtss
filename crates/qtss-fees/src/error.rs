use thiserror::Error;

#[derive(Debug, Error)]
pub enum FeeError {
    #[error("no fee schedule registered for venue '{0}'")]
    UnknownVenue(String),
    #[error("invalid fee schedule: {0}")]
    Invalid(String),
}

pub type FeeResult<T> = Result<T, FeeError>;
