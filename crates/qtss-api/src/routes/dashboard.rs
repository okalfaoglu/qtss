use axum::extract::{Extension, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use qtss_common::{log_business, QtssLogLevel};
use qtss_storage::PnlRollupRow;

use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct PnlQuery {
    pub ledger: String,
    pub bucket: String,
}

pub fn dashboard_router() -> Router<SharedState> {
    Router::new()
        .route("/dashboard/pnl", get(pnl_rollups))
}

async fn pnl_rollups(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<PnlQuery>,
) -> Result<Json<Vec<PnlRollupRow>>, String> {
    let _ = claims;
    let rows = st
        .pnl
        .list_rollups(&q.ledger, &q.bucket, 500)
        .await
        .map_err(|e| e.to_string())?;
    log_business(QtssLogLevel::Debug, "qtss_api::dashboard", "pnl_rollups");
    Ok(Json(rows))
}
