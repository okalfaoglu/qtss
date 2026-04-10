#![allow(dead_code)]
//! `GET /v2/risk` -- Faz 5 Adim (f).
//!
//! Builds the Risk HUD by snapshotting the in-memory v2 portfolio
//! engine (the same one /v2/dashboard reads) and pairing it with the
//! current `RiskConfig`. The config is `RiskConfig::defaults()` for
//! now -- the per-key system_config promotion lands with Faz 6 when
//! the live risk worker starts reading these caps from the same row
//! set the GUI Config Editor will edit (CLAUDE.md #2).
//!
//! **Drawdown history** (migration 0039):
//! - `GET /v2/risk/drawdown/history` — persisted drawdown timeseries

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Extension, Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use qtss_gui_api::{build_risk_hud, RiskHud};
use qtss_risk::RiskConfig;
use qtss_storage::DrawdownSnapshotRow;

use crate::error::ApiError;
use crate::oauth::AccessClaims;
use crate::state::SharedState;

pub fn v2_risk_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/risk", get(get_risk))
        .route("/v2/risk/drawdown/history", get(get_drawdown_history))
}

async fn get_risk(State(st): State<SharedState>) -> Result<Json<RiskHud>, ApiError> {
    let account = st.v2_dashboard.with_engine(|e| e.snapshot()).await;
    let cfg = RiskConfig::defaults();
    Ok(Json(build_risk_hud(&account, &cfg)))
}

#[derive(Debug, Deserialize)]
pub struct DrawdownQuery {
    pub exchange: Option<String>,
    pub limit: Option<i64>,
}

async fn get_drawdown_history(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<DrawdownQuery>,
) -> Result<Json<Vec<DrawdownSnapshotRow>>, ApiError> {
    let uid = Uuid::parse_str(claims.sub.trim())
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid user id in token"))?;
    let exchange = q.exchange.as_deref().unwrap_or("binance");
    let limit = q.limit.unwrap_or(500);
    let rows = st
        .account_drawdown
        .history(uid, exchange, limit)
        .await?;
    Ok(Json(rows))
}
