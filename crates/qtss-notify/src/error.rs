//! Hata türleri.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum NotifyError {
    #[error("kanal yapılandırılmadı: {0}")]
    ChannelNotConfigured(String),

    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },

    #[error("isteğe bağlı kanal hatası: {0}")]
    Transport(String),

    #[error("e-posta (SMTP): {0}")]
    Email(String),

    #[error("serileştirme: {0}")]
    Json(#[from] serde_json::Error),
}

pub type NotifyResult<T> = Result<T, NotifyError>;
