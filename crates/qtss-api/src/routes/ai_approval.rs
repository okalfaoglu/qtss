//! AI / policy approval queue (`ai_approval_requests`) — §9.1 item 6 first slice.

use axum::extract::{Extension, Path, Query, State};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use qtss_storage::AiApprovalRequestRow;

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct ListApprovalQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct CreateApprovalBody {
    /// Logical category (e.g. `strategy_intent`, `chat_reply`); default `generic`.
    pub kind: Option<String>,
    pub payload: serde_json::Value,
    pub model_hint: Option<String>,
}

#[derive(Deserialize)]
pub struct DecideBody {
    pub status: String,
    pub admin_note: Option<String>,
}

#[derive(Serialize)]
pub struct DecideResponse {
    pub updated: u64,
}

pub fn ai_approval_read_router() -> Router<SharedState> {
    Router::new().route("/ai/approval-requests", get(list_approval_requests))
}

pub fn ai_approval_submit_router() -> Router<SharedState> {
    Router::new().route("/ai/approval-requests", post(create_approval_request))
}

pub fn ai_approval_admin_router() -> Router<SharedState> {
    Router::new().route("/ai/approval-requests/{id}", patch(decide_approval_request))
}

fn parse_org(claims: &AccessClaims) -> Result<Uuid, ApiError> {
    Uuid::parse_str(claims.org_id.trim())
        .map_err(|_| ApiError::bad_request("geçersiz token org_id"))
}

async fn list_approval_requests(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<ListApprovalQuery>,
) -> Result<Json<Vec<AiApprovalRequestRow>>, ApiError> {
    let org_id = parse_org(&claims)?;
    let limit = q.limit.unwrap_or(50);
    let status = q.status.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let rows = st.ai_approval.list_for_org(org_id, status, limit).await?;
    Ok(Json(rows))
}

async fn create_approval_request(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<CreateApprovalBody>,
) -> Result<Json<AiApprovalRequestRow>, ApiError> {
    let org_id = parse_org(&claims)?;
    let uid = Uuid::parse_str(claims.sub.trim())
        .map_err(|_| ApiError::bad_request("geçersiz token sub"))?;
    let kind = body
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("generic");
    let model_hint = body
        .model_hint
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let row = st
        .ai_approval
        .insert(org_id, uid, kind, body.payload, model_hint)
        .await?;
    Ok(Json(row))
}

async fn decide_approval_request(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<DecideBody>,
) -> Result<Json<DecideResponse>, ApiError> {
    let org_id = parse_org(&claims)?;
    let admin_id = Uuid::parse_str(claims.sub.trim())
        .map_err(|_| ApiError::bad_request("geçersiz token sub"))?;
    let st_norm = body.status.trim().to_ascii_lowercase();
    if st_norm != "approved" && st_norm != "rejected" {
        return Err(ApiError::bad_request(
            "status: approved veya rejected olmalı",
        ));
    }
    let note = body
        .admin_note
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let n = st
        .ai_approval
        .decide(id, org_id, admin_id, &st_norm, note)
        .await?;
    if n == 0 {
        return Err(ApiError::not_found(
            "kayıt bulunamadı, org eşleşmedi veya durum pending değil",
        ));
    }
    Ok(Json(DecideResponse { updated: n }))
}
