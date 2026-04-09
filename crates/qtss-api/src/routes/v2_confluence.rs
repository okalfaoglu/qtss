//! `GET /v2/confluence` — Faz 7.8.
//!
//! Read-only feed for the Q-RADAR confluence panel. Returns the latest
//! `qtss_v2_confluence` row per (exchange, symbol, timeframe). Setup
//! Engine (Faz 8.0) consumes this through the same endpoint.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use qtss_storage::v2_confluence::{list_latest_v2_confluence, V2ConfluenceRow};

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct ConfluenceQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ConfluenceFeed {
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<ConfluenceEntry>,
}

#[derive(Debug, Serialize)]
pub struct ConfluenceEntry {
    pub id: Uuid,
    pub computed_at: DateTime<Utc>,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub erken_uyari: f32,
    pub guven: f32,
    pub direction: String,
    pub layer_count: i32,
    pub raw_meta: serde_json::Value,
}

pub fn v2_confluence_router() -> Router<SharedState> {
    Router::new().route("/v2/confluence", get(get_confluence))
}

async fn get_confluence(
    State(st): State<SharedState>,
    Query(q): Query<ConfluenceQuery>,
) -> Result<Json<ConfluenceFeed>, ApiError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 2_000);
    let rows = list_latest_v2_confluence(&st.pool, limit).await?;
    Ok(Json(ConfluenceFeed {
        generated_at: Utc::now(),
        entries: rows.into_iter().map(row_to_entry).collect(),
    }))
}

fn row_to_entry(row: V2ConfluenceRow) -> ConfluenceEntry {
    ConfluenceEntry {
        id: row.id,
        computed_at: row.computed_at,
        exchange: row.exchange,
        symbol: row.symbol,
        timeframe: row.timeframe,
        erken_uyari: row.erken_uyari,
        guven: row.guven,
        direction: row.direction,
        layer_count: row.layer_count,
        raw_meta: row.raw_meta,
    }
}
