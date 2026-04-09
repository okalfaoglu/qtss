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
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::Deserialize;

use qtss_gui_api::{
    build_renko, CandleBar, ChartWorkspace, DetectionAnchor, DetectionOverlay, OpenOrderOverlay,
    OpenPositionView,
};
use qtss_storage::{market_bars, DetectionRow, V2DetectionRepository};

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
    /// Pan-left cursor: when set, return `limit` bars whose
    /// `open_time < before`. The frontend passes its current oldest
    /// candle's `open_time` to walk back through history.
    pub before: Option<chrono::DateTime<chrono::Utc>>,
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

    let rows = match q.before {
        Some(before) => {
            market_bars::list_recent_bars_before(
                &st.pool, &venue, &segment, &symbol, &tf, before, limit,
            )
            .await?
        }
        None => {
            market_bars::list_recent_bars(&st.pool, &venue, &segment, &symbol, &tf, limit).await?
        }
    };

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
    let detections = detections_for(&st, &venue, &symbol, &tf).await;
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

/// Read the latest N detections for this `(venue, symbol, timeframe)`
/// from `qtss_v2_detections` and project them into the wire-shape the
/// chart panel renders. Limit comes from `system_config` so the GUI
/// can shrink it under load (CLAUDE.md #2).
async fn detections_for(
    st: &SharedState,
    venue: &str,
    symbol: &str,
    tf: &str,
) -> Vec<DetectionOverlay> {
    let limit = qtss_storage::resolve_system_u64(
        &st.pool,
        "detection",
        "chart_overlay.limit",
        "QTSS_DETECTION_CHART_OVERLAY_LIMIT",
        50,
        1,
        1000,
    )
    .await as i64;
    let repo = V2DetectionRepository::new(st.pool.clone());
    let rows = match repo.list_for_chart(venue, symbol, tf, limit).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(%e, "v2 chart: list_for_chart failed");
            return Vec::new();
        }
    };
    rows.into_iter().map(detection_row_to_overlay).collect()
}

fn detection_row_to_overlay(row: DetectionRow) -> DetectionOverlay {
    let anchors = parse_anchors(&row.anchors);
    // Faz 7.6 / A2 + A3: pull projection and sub-wave decomposition
    // out of raw_meta. Both keys are optional and the parser tolerates
    // missing fields so older rows still render their realized anchors.
    let projected_anchors = row
        .raw_meta
        .get("projected_anchors")
        .map(parse_anchors)
        .unwrap_or_default();
    let sub_wave_anchors = row
        .raw_meta
        .get("sub_wave_anchors")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(parse_anchors).collect::<Vec<_>>())
        .unwrap_or_default();
    let primary_price = anchors
        .first()
        .map(|a| a.price)
        .unwrap_or(row.invalidation_price);
    let confidence = row
        .confidence
        .and_then(Decimal::from_f32)
        .unwrap_or_else(|| Decimal::from_f32(row.structural_score).unwrap_or(Decimal::ZERO));

    DetectionOverlay {
        id: row.id.to_string(),
        kind: format!("{}/{}", row.family, row.subkind),
        label: row.subkind.clone(),
        family: row.family,
        subkind: row.subkind,
        state: row.state,
        anchor_time: row.detected_at,
        anchor_price: primary_price,
        confidence,
        invalidation_price: row.invalidation_price,
        anchors,
        projected_anchors,
        sub_wave_anchors,
    }
}

/// Parse the persisted anchor JSON into the wire shape. The orchestrator
/// writes `anchors` as a `Vec<PivotRef>` (`{ time, price, kind, idx }`)
/// — we keep only what the chart needs (time, price, optional label).
/// Robust to both string and numeric price encodings since the
/// orchestrator's serializer used to flip between them.
fn parse_anchors(value: &serde_json::Value) -> Vec<DetectionAnchor> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            let time = v
                .get("time")
                .and_then(|t| t.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&chrono::Utc))?;
            let price = v.get("price").and_then(|p| {
                if let Some(s) = p.as_str() {
                    s.parse::<Decimal>().ok()
                } else {
                    p.as_f64().and_then(Decimal::from_f64)
                }
            })?;
            let label = v
                .get("label")
                .and_then(|k| k.as_str())
                .map(|s| s.to_string())
                .or_else(|| v.get("kind").and_then(|k| k.as_str()).map(|s| s.to_string()));
            Some(DetectionAnchor { time, price, label })
        })
        .collect()
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
