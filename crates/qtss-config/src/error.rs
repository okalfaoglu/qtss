use thiserror::Error;

pub type ConfigResult<T> = Result<T, ConfigError>;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config key not found: {0}")]
    NotFound(String),

    #[error("config key '{key}' is deprecated since {since}")]
    Deprecated { key: String, since: String },

    #[error("scope not found: {scope_type}:{scope_key}")]
    ScopeNotFound {
        scope_type: String,
        scope_key: String,
    },

    #[error("validation failed for '{key}': {message}")]
    Validation { key: String, message: String },

    #[error("type mismatch for '{key}': expected {expected}, got {actual}")]
    TypeMismatch {
        key: String,
        expected: String,
        actual: String,
    },

    #[error("optimistic lock conflict on '{key}' (expected version {expected}, found {found})")]
    VersionConflict {
        key: String,
        expected: i32,
        found: i32,
    },

    #[error("audit reason is required")]
    MissingReason,

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}
