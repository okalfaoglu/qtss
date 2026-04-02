//! Public bootstrap JSON for the SPA — reads `system_config` `seed.*` (no JWT).
//!
//! **Security:** Returns the OAuth `client_secret` in plaintext so the browser can call
//! `POST /oauth/token`. Deploy behind VPN / Tailscale or set `QTSS_WEB_OAUTH_BOOTSTRAP_TOKEN`
//! and send matching header `X-QTSS-Bootstrap-Token`.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use qtss_storage::SystemConfigRepository;
use serde::Serialize;

use crate::state::SharedState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebOAuthBootstrapBody {
    pub client_id: String,
    pub client_secret: String,
    pub suggested_login_email: String,
}

fn json_config_string(v: &serde_json::Value) -> Option<String> {
    v.get("value")
        .and_then(|x| x.as_str())
        .or_else(|| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn bootstrap_gate(headers: &HeaderMap) -> Result<(), StatusCode> {
    let expected = std::env::var("QTSS_WEB_OAUTH_BOOTSTRAP_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(exp) = expected else {
        return Ok(());
    };
    let got = headers
        .get("x-qtss-bootstrap-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .unwrap_or("");
    if got == exp.as_str() {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

async fn get_web_oauth_bootstrap(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Response {
    if let Err(code) = bootstrap_gate(&headers) {
        return code.into_response();
    }

    let repo = SystemConfigRepository::new(state.pool.clone());

    let client_id = match repo.get("seed", "oauth_client_id").await {
        Ok(Some(row)) => json_config_string(&row.value).unwrap_or_else(|| "qtss-cli".to_string()),
        _ => "qtss-cli".to_string(),
    };

    let client_secret = match repo.get("seed", "oauth_client_secret").await {
        Ok(Some(row)) => json_config_string(&row.value),
        _ => None,
    };
    let Some(client_secret) = client_secret else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "oauth_client_secret_missing",
                "message": "Run qtss-seed or insert system_config seed.oauth_client_secret."
            })),
        )
            .into_response();
    };

    let suggested_login_email = match repo.get("seed", "admin_email").await {
        Ok(Some(row)) => json_config_string(&row.value).unwrap_or_else(|| "admin@localhost".to_string()),
        _ => "admin@localhost".to_string(),
    };

    Json(WebOAuthBootstrapBody {
        client_id,
        client_secret,
        suggested_login_email,
    })
    .into_response()
}

/// Mounted under `/api/v1` before JWT layer.
pub fn public_bootstrap_routes() -> Router<SharedState> {
    Router::new().route("/bootstrap/web-oauth-client", get(get_web_oauth_bootstrap))
}
