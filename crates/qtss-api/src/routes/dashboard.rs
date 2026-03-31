use axum::extract::{Extension, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde::Serialize;
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
    pub exchange: Option<String>,
    pub segment: Option<String>,
    pub symbol: Option<String>,
    pub limit: Option<i64>,
}

pub fn dashboard_router() -> Router<SharedState> {
    Router::new()
        .route("/dashboard/pnl", get(pnl_rollups))
        .route("/dashboard/pnl/equity", get(pnl_equity_curve))
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
        .list_rollups(
            org_id,
            q.ledger.trim(),
            q.bucket.trim(),
            q.exchange.as_deref().map(str::trim).filter(|s| !s.is_empty()),
            q.segment.as_deref().map(str::trim).filter(|s| !s.is_empty()),
            q.symbol
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_uppercase())
                .as_deref(),
            q.limit.unwrap_or(500).clamp(1, 2000),
        )
        .await?;
    log_business(QtssLogLevel::Debug, "qtss_api::dashboard", "pnl_rollups");
    Ok(Json(rows))
}

#[derive(Debug, Clone, Serialize)]
pub struct PnlEquityPoint {
    pub t: chrono::DateTime<chrono::Utc>,
    pub equity: rust_decimal::Decimal,
    pub realized_pnl: rust_decimal::Decimal,
    pub fees: rust_decimal::Decimal,
}

/// Naive “equity curve” from rollups: cumulative sum of (realized_pnl - fees).
async fn pnl_equity_curve(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<PnlQuery>,
) -> Result<Json<Vec<PnlEquityPoint>>, ApiError> {
    let org_id = Uuid::parse_str(&claims.org_id)
        .map_err(|_| ApiError::bad_request("geçersiz token org_id"))?;
    let mut rows = st
        .pnl
        .list_rollups(
            org_id,
            q.ledger.trim(),
            q.bucket.trim(),
            q.exchange.as_deref().map(str::trim).filter(|s| !s.is_empty()),
            q.segment.as_deref().map(str::trim).filter(|s| !s.is_empty()),
            q.symbol
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_uppercase())
                .as_deref(),
            q.limit.unwrap_or(500).clamp(1, 5000),
        )
        .await?;
    // rollups returned newest-first; for curve we want chrono order.
    rows.sort_by_key(|r| r.period_start);

    let mut equity = rust_decimal::Decimal::ZERO;
    let points = rows
        .into_iter()
        .map(|r| {
            equity += r.realized_pnl - r.fees;
            PnlEquityPoint {
                t: r.period_start,
                equity,
                realized_pnl: r.realized_pnl,
                fees: r.fees,
            }
        })
        .collect::<Vec<_>>();
    Ok(Json(points))
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
