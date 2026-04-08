#![allow(dead_code)]
//! `GET /v2/audit` -- Faz 5 Adim (k).
//!
//! Read-only projection of `audit_log` for the GUI Audit Log Viewer.
//! Behind the existing audit-read role gate so non-auditors do not
//! see the trail.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;

use qtss_gui_api::{details_preview, extract_kind, AuditEntry, AuditView};
use qtss_storage::audit_log;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    /// Optional `details->>'kind'` filter, mirrors the storage helper.
    pub kind: Option<String>,
    pub limit: Option<i64>,
}

pub fn v2_audit_router() -> Router<SharedState> {
    Router::new().route("/v2/audit", get(get_audit))
}

async fn get_audit(
    State(st): State<SharedState>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<AuditView>, ApiError> {
    let limit = q
        .limit
        .unwrap_or_else(|| env_int("QTSS_V2_AUDIT_LIMIT", 200))
        .clamp(1, 500);
    let kind = q.kind.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let rows = audit_log::list_recent(&st.pool, limit, kind).await?;

    let entries: Vec<AuditEntry> = rows
        .into_iter()
        .map(|r| AuditEntry {
            id: r.id.to_string(),
            at: r.created_at,
            request_id: r.request_id,
            user_id: r.user_id.map(|u| u.to_string()),
            org_id: r.org_id.map(|u| u.to_string()),
            method: r.method,
            path: r.path,
            status_code: r.status_code as u16,
            roles: r.roles,
            kind: extract_kind(r.details.as_ref()),
            details_preview: details_preview(r.details.as_ref()),
        })
        .collect();

    Ok(Json(AuditView {
        generated_at: Utc::now(),
        entries,
    }))
}

fn env_int(key: &str, default: i64) -> i64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parses_kind_and_limit() {
        let q: AuditQuery = serde_urlencoded::from_str("kind=config_upsert&limit=50").unwrap();
        assert_eq!(q.kind.as_deref(), Some("config_upsert"));
        assert_eq!(q.limit, Some(50));
    }
}
