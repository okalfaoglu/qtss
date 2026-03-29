//! Oturum özeti — JWT içindeki rol ve kimlik (GUI RBAC için).

use axum::extract::Extension;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub sub: String,
    pub org_id: String,
    pub roles: Vec<String>,
    /// Coarse capability strings (`qtss:read` | `qtss:ops` | `qtss:admin`); mirrors JWT after normalization.
    pub permissions: Vec<String>,
    pub azp: String,
}

pub fn session_router() -> Router<SharedState> {
    Router::new().route("/me", get(me))
}

async fn me(Extension(claims): Extension<AccessClaims>) -> Json<MeResponse> {
    Json(MeResponse {
        sub: claims.sub,
        org_id: claims.org_id,
        roles: claims.roles,
        permissions: claims.permissions,
        azp: claims.azp,
    })
}
