//! Son `audit_log` kayıtları (admin).

use axum::extract::{Extension, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::oauth::AccessClaims;
use crate::state::SharedState;
use qtss_storage::{audit_list_recent, AuditHttpListRow};

#[derive(Deserialize)]
pub struct AuditRecentParams {
    pub limit: Option<i64>,
    /// `audit_log.details->>'kind'` (ör. `user_permissions_replace`).
    pub kind: Option<String>,
}

pub fn audit_admin_router() -> Router<SharedState> {
    Router::new().route("/audit/recent", get(list_audit_recent))
}

async fn list_audit_recent(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<AuditRecentParams>,
) -> Result<Json<Vec<AuditHttpListRow>>, String> {
    let limit = q.limit.unwrap_or(100);
    let kind = q.kind.as_deref();
    let rows = audit_list_recent(&st.pool, limit, kind)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(rows))
}
