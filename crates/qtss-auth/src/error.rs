use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("user not found: {0}")]
    UserNotFound(String),
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("user is disabled: {0}")]
    UserDisabled(String),
    #[error("session expired or revoked")]
    SessionInvalid,
    #[error("permission denied: {0:?}")]
    PermissionDenied(crate::roles::Permission),
    #[error("password hashing error: {0}")]
    Hash(String),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub type AuthResult<T> = Result<T, AuthError>;
