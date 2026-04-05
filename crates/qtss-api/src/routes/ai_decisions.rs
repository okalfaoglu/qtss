//! LLM decision chain (`ai_decisions` + directives) — FAZ 7.1.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct AiDecisionsQuery {
    pub layer: Option<String>,
    pub symbol: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct TacticalDirectiveQuery {
    pub symbol: String,
}

#[derive(Deserialize)]
pub struct LinkApprovalBody {
    pub approval_request_id: Uuid,
}

#[derive(Deserialize)]
pub struct DeleteAiDecisionsQuery {
    /// Only `error` is allowed (bulk cleanup of failed LLM rows).
    pub status: String,
}

fn map_ai(e: qtss_ai::AiError) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}"))
}

pub fn ai_decisions_read_router() -> Router<SharedState> {
    Router::new()
        .route("/ai/decisions", get(list_ai_decisions))
        .route("/ai/decisions/{id}", get(get_ai_decision))
        .route("/ai/directives/tactical", get(latest_tactical_directive))
        .route("/ai/directives/portfolio", get(active_portfolio_directive))
}

pub fn ai_decisions_admin_router() -> Router<SharedState> {
    Router::new()
        .route("/ai/decisions", delete(delete_ai_decisions_by_status))
        .route("/ai/decisions/{id}/approve", post(approve_ai_decision))
        .route("/ai/decisions/{id}/reject", post(reject_ai_decision))
        .route(
            "/ai/decisions/{id}/link-approval-request",
            post(link_decision_to_approval_request),
        )
}

async fn list_ai_decisions(
    State(st): State<SharedState>,
    Query(q): Query<AiDecisionsQuery>,
) -> Result<Json<Vec<qtss_ai::storage::AiDecisionListRow>>, ApiError> {
    let limit = q.limit.unwrap_or(50);
    let rows = qtss_ai::storage::list_ai_decisions(
        &st.pool,
        q.layer.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        q.symbol.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        q.status.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        limit,
    )
    .await
    .map_err(map_ai)?;
    Ok(Json(rows))
}

async fn get_ai_decision(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<qtss_ai::storage::AiDecisionDetailRow>, ApiError> {
    let row = qtss_ai::storage::fetch_ai_decision_detail(&st.pool, id)
        .await
        .map_err(map_ai)?;
    let Some(r) = row else {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "AI decision not found"));
    };
    Ok(Json(r))
}

async fn latest_tactical_directive(
    State(st): State<SharedState>,
    Query(q): Query<TacticalDirectiveQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let sym = q.symbol.trim();
    if sym.is_empty() {
        return Err(ApiError::bad_request("symbol gerekli"));
    }
    let row = qtss_ai::storage::fetch_latest_approved_tactical(&st.pool, sym)
        .await
        .map_err(map_ai)?;
    Ok(Json(serde_json::to_value(row).unwrap_or(serde_json::json!(null))))
}

async fn active_portfolio_directive(
    State(st): State<SharedState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row = qtss_ai::storage::fetch_active_portfolio_directive(&st.pool)
        .await
        .map_err(map_ai)?;
    Ok(Json(serde_json::to_value(row).unwrap_or(serde_json::json!(null))))
}

async fn delete_ai_decisions_by_status(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<DeleteAiDecisionsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let st_trim = q.status.trim();
    if st_trim != "error" {
        return Err(ApiError::bad_request("only status=error is supported for bulk delete"));
    }
    let n = qtss_ai::storage::delete_ai_decisions_with_status(&st.pool, st_trim)
        .await
        .map_err(map_ai)?;
    Ok(Json(serde_json::json!({ "deleted": n })))
}

async fn approve_ai_decision(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let by = format!("jwt:{}", claims.sub.trim());
    let n = qtss_ai::storage::admin_approve_ai_decision(&st.pool, id, &by)
        .await
        .map_err(map_ai)?;
    if n == 0 {
        return Err(ApiError::bad_request("no pending decision updated"));
    }
    Ok(Json(serde_json::json!({ "updated": n })))
}

async fn reject_ai_decision(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let by = format!("jwt:{}", claims.sub.trim());
    let n = qtss_ai::storage::admin_reject_ai_decision(&st.pool, id, &by)
        .await
        .map_err(map_ai)?;
    if n == 0 {
        return Err(ApiError::bad_request("no pending decision updated"));
    }
    Ok(Json(serde_json::json!({ "updated": n })))
}

async fn link_decision_to_approval_request(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(body): Json<LinkApprovalBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let org_id = Uuid::parse_str(claims.org_id.trim())
        .map_err(|_| ApiError::bad_request("geçersiz token org_id"))?;
    let row = st
        .ai_approval
        .fetch_by_id_for_org(body.approval_request_id, org_id)
        .await?;
    let Some(apr) = row else {
        return Err(ApiError::not_found("approval request not found for org"));
    };
    if apr.status != "pending" {
        return Err(ApiError::bad_request(
            "approval request must be pending to link",
        ));
    }
    let n = qtss_ai::storage::set_ai_decision_approval_link(&st.pool, id, body.approval_request_id)
        .await
        .map_err(map_ai)?;
    if n == 0 {
        return Err(ApiError::bad_request(
            "AI decision not pending or id invalid",
        ));
    }
    Ok(Json(serde_json::json!({ "linked": n })))
}
