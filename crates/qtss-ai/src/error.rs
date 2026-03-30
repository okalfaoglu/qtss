//! Error type for the AI pipeline (`qtss-ai`).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("configuration: {0}")]
    Config(String),

    #[error("unknown AI provider id: {0}")]
    UnknownProvider(String),

    #[error("provider not configured: {0}")]
    ProviderNotConfigured(String),

    #[error("HTTP provider error: {0}")]
    Http(String),

    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("storage: {0}")]
    Storage(#[from] qtss_storage::StorageError),

    #[error("parse: {0}")]
    Parse(String),

    #[error("safety: {0}")]
    Safety(&'static str),

    #[error("approval / notify: {0}")]
    Notify(String),

    #[error("database: {0}")]
    Database(#[from] sqlx::Error),
}

impl AiError {
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    pub fn http(msg: impl Into<String>) -> Self {
        Self::Http(msg.into())
    }

    pub fn parse(msg: impl Into<String>) -> Self {
        Self::Parse(msg.into())
    }
}

pub type AiResult<T> = Result<T, AiError>;
