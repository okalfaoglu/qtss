//! JSON API hataları — `{"error": "..."}` + doğru HTTP durum kodu (FAZ 0.7).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use qtss_storage::StorageError;
use serde_json::json;
use std::fmt::Display;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
    /// Echo of negotiated locale for structured clients (FAZ 9.2).
    pub locale: Option<String>,
    /// Stable machine-readable key for i18n / clients (FAZ 9.2).
    pub error_key: Option<String>,
    /// Optional template arguments for `error_key` (e.g. `{"field":"symbol"}`).
    pub error_args: Option<serde_json::Value>,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            locale: None,
            error_key: None,
            error_args: None,
        }
    }

    pub fn with_locale(mut self, locale: impl Into<String>) -> Self {
        self.locale = Some(locale.into());
        self
    }

    pub fn with_error_key(mut self, key: impl Into<String>) -> Self {
        self.error_key = Some(key.into());
        self
    }

    pub fn with_error_args(mut self, args: serde_json::Value) -> Self {
        self.error_args = Some(args);
        self
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
    }
}

impl Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.status, self.message)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        ApiError::internal(e.to_string())
    }
}

impl From<StorageError> for ApiError {
    fn from(e: StorageError) -> Self {
        ApiError::internal(e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = serde_json::Map::new();
        body.insert("error".to_string(), json!(self.message));
        if let Some(loc) = self.locale {
            body.insert("locale".to_string(), json!(loc));
        }
        if let Some(k) = self.error_key {
            body.insert("error_key".to_string(), json!(k));
        }
        if let Some(a) = self.error_args {
            body.insert("error_args".to_string(), a);
        }
        (self.status, Json(body)).into_response()
    }
}
