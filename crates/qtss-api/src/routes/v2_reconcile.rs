//! `GET /v2/reconcile/reports` — persisted reconciliation snapshots.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Extension, Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use qtss_storage::ReconcileReportRow;

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct ReportsQuery {
    pub venue: Option<String>,
    pub limit: Option<i64>,
}

pub fn v2_reconcile_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/reconcile/reports", get(list_reports))
        .route("/v2/reconcile/reports/latest", get(latest_reports))
        .route("/v2/reconcile/reports/{id}", get(get_report))
}

async fn list_reports(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<ReportsQuery>,
) -> Result<Json<Vec<ReconcileReportRow>>, ApiError> {
    let uid = Uuid::parse_str(claims.sub.trim())
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid user id in token"))?;
    let limit = q.limit.unwrap_or(20);
    let rows = st
        .reconcile_reports
        .list(uid, q.venue.as_deref(), limit)
        .await?;
    Ok(Json(rows))
}

async fn latest_reports(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
) -> Result<Json<Vec<ReconcileReportRow>>, ApiError> {
    let uid = Uuid::parse_str(claims.sub.trim())
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid user id in token"))?;
    let rows = st.reconcile_reports.latest_per_venue(uid).await?;
    Ok(Json(rows))
}

async fn get_report(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Path(id): Path<i64>,
) -> Result<Json<ReconcileReportRow>, ApiError> {
    let row = st
        .reconcile_reports
        .get_by_id(id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "reconcile report not found"))?;
    Ok(Json(row))
}
