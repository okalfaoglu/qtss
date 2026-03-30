//! Oturum özeti — JWT içindeki rol ve kimlik (GUI RBAC için).
//! `preferred_locale`: migration `0045` kolonu; `GET /me` + `PATCH /me/locale` (FAZ 9.3).

use axum::extract::{Extension, State};
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub sub: String,
    pub org_id: String,
    pub roles: Vec<String>,
    /// Etkin yetenekler: JWT (rol / claim) + `user_permissions` birleşimi (`require_jwt`).
    pub permissions: Vec<String>,
    pub azp: String,
    /// `users.preferred_locale` (`en` \| `tr`) veya JSON `null`.
    pub preferred_locale: Option<String>,
}

pub fn session_router() -> Router<SharedState> {
    Router::new()
        .route("/me", get(me))
        .route("/me/locale", patch(patch_me_locale))
}

fn user_id_from_claims(claims: &AccessClaims) -> Result<Uuid, ApiError> {
    Uuid::parse_str(claims.sub.trim()).map_err(|_| {
        ApiError::bad_request("invalid token subject")
            .with_error_key("session.invalid_sub")
    })
}

async fn me(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
) -> Result<Json<MeResponse>, ApiError> {
    let uid = user_id_from_claims(&claims)?;
    let loc = st.users.get_preferred_locale(uid).await?;
    Ok(Json(MeResponse {
        sub: claims.sub,
        org_id: claims.org_id,
        roles: claims.roles,
        permissions: claims.permissions,
        azp: claims.azp,
        preferred_locale: loc,
    }))
}

#[derive(Debug, Deserialize)]
pub struct PatchPreferredLocaleBody {
    pub preferred_locale: JsonValue,
}

fn normalized_locale_from_json(v: &JsonValue) -> Result<Option<String>, ApiError> {
    if v.is_null() {
        return Ok(None);
    }
    let Some(s) = v.as_str() else {
        return Err(
            ApiError::bad_request("preferred_locale must be a string or null")
                .with_error_key("session.locale_invalid_type"),
        );
    };
    let t = s.trim().to_lowercase();
    if t == "en" || t == "tr" {
        return Ok(Some(t));
    }
    Err(
        ApiError::bad_request("preferred_locale must be en or tr")
            .with_error_key("session.locale_invalid_value"),
    )
}

async fn patch_me_locale(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<PatchPreferredLocaleBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let uid = user_id_from_claims(&claims)?;
    let locale = normalized_locale_from_json(&body.preferred_locale)?;
    st.users.set_preferred_locale(uid, locale.as_deref()).await?;
    Ok(Json(serde_json::json!({ "ok": true, "preferred_locale": locale })))
}
