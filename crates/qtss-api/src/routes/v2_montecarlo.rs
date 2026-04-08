#![allow(dead_code)]
//! `GET /v2/montecarlo/{venue}/{symbol}/{tf}` -- Faz 5 Adim (e).
//!
//! Calibrates a GBM-style simulator on the most recent N closes from
//! `qtss_storage::market_bars` and returns a fan-chart payload (one
//! band per requested percentile, anchor implicit at step 0).
//!
//! All knobs (window, horizon, paths, default percentile set) are
//! resolved from env today as a placeholder for the future
//! `system_config` rows -- nothing about the projection is hardcoded
//! (CLAUDE.md #2). Per-request overrides are accepted on the query.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::Deserialize;

use qtss_gui_api::{build_montecarlo_fan, MonteCarloFan};
use qtss_storage::market_bars;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct MonteCarloQuery {
    /// How many recent bars to calibrate against.
    pub window: Option<i64>,
    /// Bars forward to project.
    pub horizon: Option<u32>,
    /// Independent paths to simulate.
    pub paths: Option<u32>,
    /// CSV percentile list (e.g. "5,25,50,75,95"). Falls back to env.
    pub percentiles: Option<String>,
    /// Deterministic seed; default 0.
    pub seed: Option<u64>,
    pub segment: Option<String>,
}

pub fn v2_montecarlo_router() -> Router<SharedState> {
    Router::new().route(
        "/v2/montecarlo/{venue}/{symbol}/{tf}",
        get(get_montecarlo),
    )
}

async fn get_montecarlo(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<MonteCarloQuery>,
) -> Result<Json<MonteCarloFan>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "spot".to_string());
    let window = q
        .window
        .unwrap_or_else(|| env_int("QTSS_V2_MONTECARLO_WINDOW", 400))
        .clamp(2, 5_000);
    let horizon = (q
        .horizon
        .unwrap_or_else(|| env_int("QTSS_V2_MONTECARLO_HORIZON", 30) as u32))
        .clamp(1, 5_000);
    let paths = (q
        .paths
        .unwrap_or_else(|| env_int("QTSS_V2_MONTECARLO_PATHS", 500) as u32))
        .clamp(1, 20_000);
    let pct_csv = q
        .percentiles
        .unwrap_or_else(|| env_string("QTSS_V2_MONTECARLO_PERCENTILES", "5,25,50,75,95"));
    let percentiles = parse_percentiles(&pct_csv);
    if percentiles.is_empty() {
        return Err(ApiError::bad_request(
            "percentiles must be a comma-separated list of values in 0..=100".to_string(),
        ));
    }
    let seed = q.seed.unwrap_or(0);

    let rows =
        market_bars::list_recent_bars(&st.pool, &venue, &segment, &symbol, &tf, window).await?;
    let mut closes: Vec<Decimal> = rows.into_iter().map(|r| r.close).collect();
    closes.reverse();
    let anchor = closes.last().copied().unwrap_or(Decimal::ZERO);

    let bands = build_montecarlo_fan(&closes, anchor, horizon, paths, &percentiles, seed);

    Ok(Json(MonteCarloFan {
        generated_at: chrono::Utc::now(),
        venue,
        symbol,
        timeframe: tf,
        horizon_bars: horizon,
        paths_simulated: paths,
        anchor_price: anchor,
        bands,
    }))
}

fn parse_percentiles(csv: &str) -> Vec<u8> {
    csv.split(',')
        .filter_map(|s| s.trim().parse::<u8>().ok())
        .filter(|p| *p <= 100)
        .collect()
}

fn env_int(key: &str, default: i64) -> i64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn env_string(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parses_full_param_set() {
        let q: MonteCarloQuery =
            serde_urlencoded::from_str("window=300&horizon=20&paths=400&percentiles=5,50,95&seed=42")
                .unwrap();
        assert_eq!(q.window, Some(300));
        assert_eq!(q.horizon, Some(20));
        assert_eq!(q.paths, Some(400));
        assert_eq!(q.percentiles.as_deref(), Some("5,50,95"));
        assert_eq!(q.seed, Some(42));
    }

    #[test]
    fn parse_percentiles_filters_invalid() {
        let p = parse_percentiles("5, 50, 95, 200, foo");
        assert_eq!(p, vec![5, 50, 95]);
    }
}
