//! `GET /v2/pivots/{venue}/{symbol}/{tf}` — multi-level ZigZag pivots for chart overlay.
//!
//! Returns all four cascaded pivot levels (L0 micro → L3 macro) the
//! `qtss-pivots` engine produces. The chart panel renders each level as
//! a separate line series so the operator can toggle L0/L1/L2/L3
//! independently (CLAUDE.md #2 — per-level style is driven by config,
//! not hardcoded).
//!
//! Data source: `pivot_cache` table (authoritative, written by both the
//! live detection orchestrator and the hourly historical backfill
//! worker). Bar indexes are globally consistent so the frontend can
//! join pivots to candles by `open_time`.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use qtss_storage::pivot_cache::list_pivot_cache;

use crate::error::ApiError;
use crate::state::SharedState;

const LEVELS: [&str; 4] = ["L0", "L1", "L2", "L3"];

#[derive(Debug, Deserialize)]
pub struct PivotsQuery {
    pub segment: Option<String>,
    pub limit: Option<i64>,
    /// Optional comma-separated level filter, e.g. `?levels=L1,L2`.
    /// Empty/missing → return all four levels.
    pub levels: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PivotPoint {
    pub bar_index: i64,
    pub open_time: DateTime<Utc>,
    pub price: Decimal,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swing_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PivotLevelSeries {
    pub level: String,
    pub points: Vec<PivotPoint>,
}

#[derive(Debug, Serialize)]
pub struct PivotsResponse {
    pub venue: String,
    pub symbol: String,
    pub timeframe: String,
    pub levels: Vec<PivotLevelSeries>,
}

pub fn v2_pivots_router() -> Router<SharedState> {
    Router::new().route("/v2/pivots/{venue}/{symbol}/{tf}", get(get_pivots))
}

async fn get_pivots(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<PivotsQuery>,
) -> Result<Json<PivotsResponse>, ApiError> {
    let _segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(2_000).clamp(1, 20_000);

    // Parse level filter (lookup-style — no if/else chain per CLAUDE.md #1)
    let requested_levels: Vec<&str> = match q.levels.as_deref() {
        None | Some("") => LEVELS.to_vec(),
        Some(csv) => csv
            .split(',')
            .map(str::trim)
            .filter(|s| LEVELS.contains(s))
            .collect(),
    };

    let mut out: Vec<PivotLevelSeries> = Vec::with_capacity(requested_levels.len());
    for level in requested_levels {
        let rows = list_pivot_cache(&st.pool, &venue, &symbol, &tf, level, limit)
            .await
            .map_err(|e| {
                ApiError::new(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("pivot_cache read failed: {e}"),
                )
            })?;
        let points = rows
            .into_iter()
            .map(|r| PivotPoint {
                bar_index: r.bar_index,
                open_time: r.open_time,
                price: r.price,
                kind: r.kind,
                swing_type: r.swing_type,
            })
            .collect();
        out.push(PivotLevelSeries {
            level: level.to_string(),
            points,
        });
    }

    Ok(Json(PivotsResponse {
        venue,
        symbol,
        timeframe: tf,
        levels: out,
    }))
}
