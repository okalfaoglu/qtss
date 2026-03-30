//! Session summary — JWT role and identity (GUI RBAC).

use axum::extract::{Extension, State};
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;
use crate::locale::NegotiatedLocale;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub sub: String,
    pub org_id: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    pub azp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_locale: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PatchLocaleBody {
    pub preferred_locale: Option<String>,
}

pub fn session_router() -> Router<SharedState> {
    Router::new()
        .route("/me", get(me))
        .route("/me/locale", patch(patch_locale))
}

fn parse_user_locale(input: &str) -> Result<String, ApiError> {
    let t = input.trim().to_lowercase();
    match t.as_str() {
        "" => Err(ApiError::bad_request("locale must not be empty")),
        "en" | "en-us" | "en-gb" => Ok("en".into()),
        "tr" | "tr-tr" => Ok("tr".into()),
        _ => Err(ApiError::bad_request("unsupported locale: use en or tr")),
    }
}

async fn me(
    State(state): State<SharedState>,
    Extension(claims): Extension<AccessClaims>,
) -> Result<Json<MeResponse>, ApiError> {
    let uid = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("invalid token sub"))?;
    let preferred_locale: Option<String> = sqlx::query_scalar(
        "SELECT preferred_locale FROM users WHERE id = $1",
    )
    .bind(uid)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(MeResponse {
        sub: claims.sub,
        org_id: claims.org_id,
        roles: claims.roles,
        permissions: claims.permissions,
        azp: claims.azp,
        preferred_locale: preferred_locale.filter(|s| !s.trim().is_empty()),
    }))
}

async fn patch_locale(
    State(state): State<SharedState>,
    Extension(claims): Extension<AccessClaims>,
    Extension(loc): Extension<NegotiatedLocale>,
    Json(body): Json<PatchLocaleBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let l = loc.0.clone();
    let uid = Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::bad_request("invalid token sub").with_locale(l.clone()))?;

    let normalized: Option<String> = match &body.preferred_locale {
        None => None,
        Some(s) if s.trim().is_empty() => None,
        Some(s) => Some(parse_user_locale(s).map_err(|e| e.with_locale(l.clone()))?),
    };

    sqlx::query("UPDATE users SET preferred_locale = $1 WHERE id = $2")
        .bind(&normalized)
        .bind(uid)
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({ "preferred_locale": normalized })))
}
