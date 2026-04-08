#![allow(dead_code)]
//! `GET /v2/ai-decisions` -- Faz 5 Adim (j).
//!
//! Read-only projection of `ai_approval_requests` for the AI Decisions
//! card. Mutations stay on the existing `/ai/approval/*` routes
//! (ops/admin roles) so the role boundary stays clean.

use axum::extract::{Extension, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use qtss_gui_api::{
    payload_preview, AiDecisionEntry, AiDecisionStatus, AiDecisionsView,
};
use qtss_storage::AiApprovalRequestRow;

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct AiDecisionsQuery {
    /// Optional status filter (`pending` / `approved` / `rejected`).
    pub status: Option<String>,
    pub limit: Option<i64>,
}

pub fn v2_ai_decisions_router() -> Router<SharedState> {
    Router::new().route("/v2/ai-decisions", get(get_ai_decisions))
}

async fn get_ai_decisions(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<AiDecisionsQuery>,
) -> Result<Json<AiDecisionsView>, ApiError> {
    let org_id = Uuid::parse_str(claims.org_id.trim())
        .map_err(|_| ApiError::bad_request("invalid token org_id"))?;
    let limit = q
        .limit
        .unwrap_or_else(|| env_int("QTSS_V2_AI_DECISIONS_LIMIT", 100))
        .clamp(1, 200);
    let status_filter = q.status.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let rows = st
        .ai_approval
        .list_for_org(org_id, status_filter, limit)
        .await?;

    let entries: Vec<AiDecisionEntry> = rows.into_iter().map(row_to_entry).collect();

    Ok(Json(AiDecisionsView {
        generated_at: Utc::now(),
        entries,
    }))
}

fn row_to_entry(r: AiApprovalRequestRow) -> AiDecisionEntry {
    AiDecisionEntry {
        id: r.id.to_string(),
        kind: r.kind,
        status: AiDecisionStatus::parse(&r.status),
        model_hint: r.model_hint,
        payload_preview: payload_preview(&r.payload),
        admin_note: r.admin_note,
        created_at: r.created_at,
        decided_at: r.decided_at,
    }
}

fn env_int(key: &str, default: i64) -> i64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parses_status_and_limit() {
        let q: AiDecisionsQuery =
            serde_urlencoded::from_str("status=pending&limit=50").unwrap();
        assert_eq!(q.status.as_deref(), Some("pending"));
        assert_eq!(q.limit, Some(50));
    }
}
