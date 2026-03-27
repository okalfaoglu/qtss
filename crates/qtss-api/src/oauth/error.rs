use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

/// OAuth 2.0 hata gövdesi (RFC 6749 §5.2).
#[derive(Debug, Serialize)]
pub struct OAuthErrorBody {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}

pub type OAuthErr = (StatusCode, Json<OAuthErrorBody>);

pub fn invalid_request(desc: impl Into<String>) -> OAuthErr {
    (
        StatusCode::BAD_REQUEST,
        Json(OAuthErrorBody {
            error: "invalid_request".into(),
            error_description: Some(desc.into()),
        }),
    )
}

pub fn invalid_client(desc: impl Into<String>) -> OAuthErr {
    (
        StatusCode::UNAUTHORIZED,
        Json(OAuthErrorBody {
            error: "invalid_client".into(),
            error_description: Some(desc.into()),
        }),
    )
}

pub fn invalid_grant(desc: impl Into<String>) -> OAuthErr {
    (
        StatusCode::BAD_REQUEST,
        Json(OAuthErrorBody {
            error: "invalid_grant".into(),
            error_description: Some(desc.into()),
        }),
    )
}

pub fn unsupported_grant_type() -> OAuthErr {
    (
        StatusCode::BAD_REQUEST,
        Json(OAuthErrorBody {
            error: "unsupported_grant_type".into(),
            error_description: Some("grant_type desteklenmiyor".into()),
        }),
    )
}

pub fn server_error(desc: impl Into<String>) -> OAuthErr {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(OAuthErrorBody {
            error: "server_error".into(),
            error_description: Some(desc.into()),
        }),
    )
}

pub fn invalid_token(desc: impl Into<String>) -> OAuthErr {
    (
        StatusCode::UNAUTHORIZED,
        Json(OAuthErrorBody {
            error: "invalid_token".into(),
            error_description: Some(desc.into()),
        }),
    )
}

/// RBAC: geçerli token var ancak rol yetersiz (OAuth 2.0 `insufficient_scope` benzeri).
pub struct Forbidden {
    pub description: String,
}

impl Forbidden {
    pub fn new(desc: impl Into<String>) -> Self {
        Self {
            description: desc.into(),
        }
    }
}

impl IntoResponse for Forbidden {
    fn into_response(self) -> Response {
        (
            StatusCode::FORBIDDEN,
            Json(OAuthErrorBody {
                error: "insufficient_scope".into(),
                error_description: Some(self.description),
            }),
        )
            .into_response()
    }
}

pub struct UnauthorizedBearer;

impl IntoResponse for UnauthorizedBearer {
    fn into_response(self) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            [(
                axum::http::header::WWW_AUTHENTICATE,
                r#"Bearer error="invalid_token""#,
            )],
            Json(OAuthErrorBody {
                error: "invalid_token".into(),
                error_description: Some("Geçerli Bearer access_token gerekli".into()),
            }),
        )
            .into_response()
    }
}
