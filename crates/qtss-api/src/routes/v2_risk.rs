#![allow(dead_code)]
//! `GET /v2/risk` -- Faz 5 Adim (f).
//!
//! Builds the Risk HUD by snapshotting the in-memory v2 portfolio
//! engine (the same one /v2/dashboard reads) and pairing it with the
//! current `RiskConfig`. The config is `RiskConfig::defaults()` for
//! now -- the per-key system_config promotion lands with Faz 6 when
//! the live risk worker starts reading these caps from the same row
//! set the GUI Config Editor will edit (CLAUDE.md #2).

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

use qtss_gui_api::{build_risk_hud, RiskHud};
use qtss_risk::RiskConfig;

use crate::error::ApiError;
use crate::state::SharedState;

pub fn v2_risk_router() -> Router<SharedState> {
    Router::new().route("/v2/risk", get(get_risk))
}

async fn get_risk(State(st): State<SharedState>) -> Result<Json<RiskHud>, ApiError> {
    let account = st.v2_dashboard.with_engine(|e| e.snapshot()).await;
    let cfg = RiskConfig::defaults();
    Ok(Json(build_risk_hud(&account, &cfg)))
}
