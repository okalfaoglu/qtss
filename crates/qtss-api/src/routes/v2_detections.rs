//! `GET /v2/detections` — Faz 7 Adım 5.
//!
//! Read-only feed for the v2 Detections panel. All filters are
//! optional so the GUI can compose them however it wants
//! (exchange/symbol/timeframe/family/state/mode + limit). Reads
//! straight from `qtss_v2_detections` via `V2DetectionRepository`.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use qtss_storage::{DetectionFilter, DetectionRow, V2DetectionRepository};

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct DetectionsQuery {
    pub exchange: Option<String>,
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    pub family: Option<String>,
    pub state: Option<String>,
    pub mode: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct DetectionsFeed {
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<DetectionEntry>,
}

#[derive(Debug, Serialize)]
pub struct DetectionEntry {
    pub id: Uuid,
    pub detected_at: DateTime<Utc>,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub family: String,
    pub subkind: String,
    pub state: String,
    pub structural_score: f32,
    pub confidence: Option<f32>,
    pub invalidation_price: Decimal,
    pub validated_at: Option<DateTime<Utc>>,
    pub mode: String,
    pub channel_scores: Option<serde_json::Value>,
}

pub fn v2_detections_router() -> Router<SharedState> {
    Router::new().route("/v2/detections", get(get_detections))
}

async fn get_detections(
    State(st): State<SharedState>,
    Query(q): Query<DetectionsQuery>,
) -> Result<Json<DetectionsFeed>, ApiError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1_000);
    let repo = V2DetectionRepository::new(st.pool.clone());
    let rows = repo
        .list_filtered(DetectionFilter {
            exchange: q.exchange.as_deref(),
            symbol: q.symbol.as_deref(),
            timeframe: q.timeframe.as_deref(),
            family: q.family.as_deref(),
            state: q.state.as_deref(),
            mode: q.mode.as_deref(),
            limit,
        })
        .await?;

    Ok(Json(DetectionsFeed {
        generated_at: Utc::now(),
        entries: rows.into_iter().map(row_to_entry).collect(),
    }))
}

fn row_to_entry(row: DetectionRow) -> DetectionEntry {
    DetectionEntry {
        id: row.id,
        detected_at: row.detected_at,
        exchange: row.exchange,
        symbol: row.symbol,
        timeframe: row.timeframe,
        family: row.family,
        subkind: row.subkind,
        state: row.state,
        structural_score: row.structural_score,
        confidence: row.confidence,
        invalidation_price: row.invalidation_price,
        validated_at: row.validated_at,
        mode: row.mode,
        channel_scores: row.channel_scores,
    }
}
