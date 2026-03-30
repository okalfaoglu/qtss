use axum::extract::{Extension, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use qtss_common::{log_business, QtssLogLevel};
use qtss_storage::{PnlRebuildStats, PnlRollupRow};

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct PnlQuery {
    pub ledger: String,
    pub bucket: String,
}

pub fn dashboard_router() -> Router<SharedState> {
    Router::new().route("/dashboard/pnl", get(pnl_rollups))
}

pub fn dashboard_admin_router() -> Router<SharedState> {
    Router::new().route("/dashboard/pnl/rebuild", post(pnl_rebuild_live))
}

async fn pnl_rollups(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<PnlQuery>,
) -> Result<Json<Vec<PnlRollupRow>>, ApiError> {
    let org_id = Uuid::parse_str(&claims.org_id)
        .map_err(|_| ApiError::bad_request("geçersiz token org_id"))?;
    let rows = st
        .pnl
        .list_rollups(org_id, &q.ledger, &q.bucket, 500)
        .await?;
    log_business(QtssLogLevel::Debug, "qtss_api::dashboard", "pnl_rollups");
    Ok(Json(rows))
}

async fn pnl_rebuild_live(
    State(st): State<SharedState>,
) -> Result<Json<PnlRebuildStats>, ApiError> {
    let stats = st.pnl.rebuild_live_rollups_from_exchange_orders().await?;
    log_business(
        QtssLogLevel::Info,
        "qtss_api::dashboard",
        "pnl_rebuild_live",
    );
    Ok(Json(stats))
}
