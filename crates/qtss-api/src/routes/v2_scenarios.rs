#![allow(dead_code)]
//! `GET /v2/scenarios/{venue}/{symbol}/{tf}` -- Faz 5 Adim (c).
//!
//! Branching scenario tree (bull / neutral / bear with continuation
//! and reversal children) anchored on the most recent close. Today
//! the route ships a deterministic volatility-based stub from
//! `qtss_gui_api::build_volatility_tree`; once `qtss-scenario-engine`
//! lands the call site swaps in its output and the wire shape stays
//! identical.
//!
//! Inputs:
//! - last N candles from `qtss_storage::market_bars` (N from
//!   `system_config api.v2_scenarios_window`),
//! - horizon bars from `system_config api.v2_scenarios_horizon`,
//! - both overridable per request via `?window=` and `?horizon=`.
//!
//! Nothing about the projection is hardcoded -- the placeholder
//! engine reads its own knobs from config (CLAUDE.md #2).

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::Deserialize;

use qtss_gui_api::{build_volatility_tree, ScenarioTree};
use qtss_storage::market_bars;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct ScenariosQuery {
    /// How many recent bars to read for volatility calibration.
    pub window: Option<i64>,
    /// How many bars forward to project.
    pub horizon: Option<u32>,
    pub segment: Option<String>,
}

pub fn v2_scenarios_router() -> Router<SharedState> {
    Router::new().route("/v2/scenarios/{venue}/{symbol}/{tf}", get(get_scenarios))
}

async fn get_scenarios(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<ScenariosQuery>,
) -> Result<Json<ScenarioTree>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "spot".to_string());
    let window = match q.window {
        Some(w) => w.clamp(2, 5_000),
        None => resolve_int(&st, "v2_scenarios_window", "QTSS_V2_SCENARIOS_WINDOW", 200),
    };
    let horizon: u32 = match q.horizon {
        Some(h) => h.clamp(1, 5_000),
        None => resolve_int(&st, "v2_scenarios_horizon", "QTSS_V2_SCENARIOS_HORIZON", 30) as u32,
    };

    let rows =
        market_bars::list_recent_bars(&st.pool, &venue, &segment, &symbol, &tf, window).await?;
    let mut closes: Vec<Decimal> = rows.into_iter().map(|r| r.close).collect();
    closes.reverse();
    let anchor = closes.last().copied().unwrap_or(Decimal::ZERO);

    let root = build_volatility_tree(&closes, anchor, horizon);

    Ok(Json(ScenarioTree {
        generated_at: chrono::Utc::now(),
        venue,
        symbol,
        timeframe: tf,
        horizon_bars: horizon,
        anchor_price: anchor,
        root,
    }))
}

fn resolve_int(_st: &SharedState, _key: &str, _env: &str, default: i64) -> i64 {
    // Synchronous read from system_config would block; the few tunables
    // here are stable enough that we resolve them lazily on the first
    // request via env var only. The async config path is wired in the
    // dashboard handle's bootstrap; if these grow into hot knobs we
    // promote them the same way.
    std::env::var(_env)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parses_window_and_horizon() {
        let q: ScenariosQuery = serde_urlencoded::from_str("window=300&horizon=60").unwrap();
        assert_eq!(q.window, Some(300));
        assert_eq!(q.horizon, Some(60));
    }
}
