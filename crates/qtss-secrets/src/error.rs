use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("secret not found: {0}")]
    NotFound(String),
    #[error("secret already exists: {0}")]
    AlreadyExists(String),
    #[error("kek version {0} unknown — cannot unwrap")]
    UnknownKekVersion(i32),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub type SecretResult<T> = Result<T, SecretError>;
