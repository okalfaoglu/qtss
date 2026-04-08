#![allow(dead_code)]
//! `GET /v2/chart/{venue}/{symbol}/{tf}` -- Faz 5 Adim (b).
//!
//! Single round-trip chart workspace payload: candles + renko bricks +
//! pattern overlays + open positions + open orders. Designed so the
//! React chart panel can flip between candle and renko views without
//! refetching.
//!
//! ## Data sources today
//!
//! - **Candles**: `qtss_storage::market_bars::list_recent_bars` --
//!   the canonical OHLCV table. Segment defaults to `spot` (override
//!   via `?segment=`).
//! - **Renko**: `qtss_gui_api::build_renko` over the same candles.
//!   Brick size is resolved from `system_config`
//!   (`api.v2_chart_renko_brick_pct`) -- nothing hardcoded
//!   (CLAUDE.md #2). Frontend can override per request via
//!   `?brick_pct=` query for ad-hoc experimentation.
//! - **Positions**: from the in-memory `V2DashboardHandle` engine
//!   (the same one `/v2/dashboard` reads), filtered to the symbol.
//! - **Detections** + **Open orders**: stubbed empty for now -- the
//!   v2 detection registry and v2 open-order book do not exist yet.
//!   The wire shape is in place so adding them is a one-line splice.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::Deserialize;

use qtss_gui_api::{
    build_renko, CandleBar, ChartWorkspace, DetectionOverlay, OpenOrderOverlay, OpenPositionView,
};
use qtss_storage::market_bars;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct ChartQuery {
    /// Number of candles to return (newest first from DB, then
    /// reversed to chronological for the wire).
    pub limit: Option<i64>,
    /// Override the configured renko brick percentage. Useful for
    /// quick visual experiments without touching `system_config`.
    pub brick_pct: Option<Decimal>,
    /// Defaults to `spot` -- the only segment v2 wires today.
    pub segment: Option<String>,
}

pub fn v2_chart_router() -> Router<SharedState> {
    Router::new().route("/v2/chart/{venue}/{symbol}/{tf}", get(get_chart))
}

async fn get_chart(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<ChartQuery>,
) -> Result<Json<ChartWorkspace>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "spot".to_string());
    let limit = q.limit.unwrap_or(500).clamp(1, 5_000);

    let rows =
        market_bars::list_recent_bars(&st.pool, &venue, &segment, &symbol, &tf, limit).await?;

    // DB returns newest-first; wire needs chronological for renko.
    let mut candles: Vec<CandleBar> = rows
        .into_iter()
        .map(|r| CandleBar {
            open_time: r.open_time,
            open: r.open,
            high: r.high,
            low: r.low,
            close: r.close,
            volume: r.volume,
        })
        .collect();
    candles.reverse();

    let brick_pct = match q.brick_pct {
        Some(p) => p,
        None => resolve_brick_pct(&st).await,
    };
    let brick_size = match candles.last() {
        Some(last) => last.close * brick_pct,
        None => Decimal::ZERO,
    };
    let renko = build_renko(&candles, brick_size);

    let positions = positions_for(&st, &symbol).await;
    let detections: Vec<DetectionOverlay> = Vec::new();
    let open_orders: Vec<OpenOrderOverlay> = Vec::new();

    Ok(Json(ChartWorkspace {
        generated_at: chrono::Utc::now(),
        venue,
        symbol,
        timeframe: tf,
        candles,
        renko,
        detections,
        positions,
        open_orders,
    }))
}

/// Pull the renko brick percentage from `system_config`. Falls back
/// to a tiny conservative default only when the row is missing AND
/// the env var is unset -- not a "magic constant" but the
/// bootstrap-time fallback that the operator can override.
async fn resolve_brick_pct(st: &SharedState) -> Decimal {
    let raw = qtss_storage::resolve_system_string(
        &st.pool,
        "api",
        "v2_chart_renko_brick_pct",
        "QTSS_V2_CHART_RENKO_BRICK_PCT",
        "0.005",
    )
    .await;
    raw.parse::<Decimal>().unwrap_or_else(|_| Decimal::new(5, 3))
}

async fn positions_for(st: &SharedState, symbol: &str) -> Vec<OpenPositionView> {
    let snap = st.v2_dashboard.snapshot().await;
    snap.open_positions
        .into_iter()
        .filter(|p| p.symbol == symbol)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn brick_pct_query_overrides_config() {
        // Smoke: just confirm the parser path -- the route handler
        // itself needs an HTTP harness, which we cover at the
        // integration tier.
        let q: ChartQuery = serde_urlencoded::from_str("brick_pct=0.01&limit=100").unwrap();
        assert_eq!(q.brick_pct, Some(dec!(0.01)));
        assert_eq!(q.limit, Some(100));
    }
}
