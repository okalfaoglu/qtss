//! `GET /v2/elliott-early/{venue}/{symbol}/{tf}` — early-wave Elliott
//! markers (FAZ 25 PR-25A).
//!
//! Returns nascent / forming / extended impulse detections persisted by
//! the engine writer under `pattern_family = 'elliott_early'`. The
//! frontend (LuxAlgoChart) overlays these as small triangle markers
//! on top of the LuxAlgo motive/abc/triangle output — they do not
//! interfere with the existing `/v2/elliott-db` response shape.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Serialize)]
pub struct EarlyMarker {
    pub slot: i16,
    pub subkind: String,           // impulse_nascent_bull / forming / extended
    pub stage: String,             // "nascent" | "forming" | "extended"
    pub direction: i16,            // +1 bull, -1 bear
    pub start_bar: i64,
    pub end_bar: i64,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: chrono::DateTime<chrono::Utc>,
    pub anchors: Value,            // [{bar_index, price, time, direction}, ...]
    pub score: f64,                // fib-snap quality 0..1
    pub w3_extension: f64,         // multiple of W1 (1.0 = equal, 1.618 = canonical)
    pub invalidation_price: f64,   // p0 — Wave 1 start
}

#[derive(Debug, Serialize)]
pub struct EarlyResponse {
    pub venue: String,
    pub symbol: String,
    pub timeframe: String,
    pub markers: Vec<EarlyMarker>,
}

pub fn v2_elliott_early_router() -> Router<SharedState> {
    Router::new().route(
        "/v2/elliott-early/{exchange}/{symbol}/{tf}",
        get(get_elliott_early),
    )
}

#[derive(Debug, Deserialize)]
struct QueryFull {
    /// "spot" | "futures" — mirrors /v2/elliott-db convention.
    segment: Option<String>,
    /// Cap recent markers (default 200, applied after time DESC sort).
    limit: Option<i64>,
}

async fn get_elliott_early(
    State(state): State<SharedState>,
    Path((exchange, symbol, tf)): Path<(String, String, String)>,
    axum::extract::Query(q): axum::extract::Query<QueryFull>,
) -> Result<impl IntoResponse, ApiError> {
    let limit = q.limit.unwrap_or(200).clamp(10, 2000);
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let venue = format!("{}.{}", exchange, segment);
    let pool = &state.pool;

    let rows = sqlx::query(
        r#"SELECT slot, subkind, direction,
                  start_bar, end_bar, start_time, end_time,
                  anchors, raw_meta
             FROM detections
            WHERE exchange = $1 AND segment = $2
              AND symbol   = $3 AND timeframe = $4
              AND mode = 'live'
              AND pattern_family = 'elliott_early'
            ORDER BY end_time DESC
            LIMIT $5"#,
    )
    .bind(&exchange)
    .bind(&segment)
    .bind(&symbol)
    .bind(&tf)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("elliott-early query failed: {e}"),
        )
    })?;

    let markers: Vec<EarlyMarker> = rows
        .into_iter()
        .map(|r| {
            let raw_meta: Value = r.try_get("raw_meta").unwrap_or(Value::Null);
            let stage = raw_meta
                .get("stage")
                .and_then(|v| v.as_str())
                .unwrap_or("nascent")
                .to_string();
            let score = raw_meta
                .get("score")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let w3_extension = raw_meta
                .get("w3_extension")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let invalidation_price = raw_meta
                .get("invalidation_price")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            EarlyMarker {
                slot: r.get("slot"),
                subkind: r.get("subkind"),
                stage,
                direction: r.get("direction"),
                start_bar: r.get("start_bar"),
                end_bar: r.get("end_bar"),
                start_time: r.get("start_time"),
                end_time: r.get("end_time"),
                anchors: r.try_get("anchors").unwrap_or(Value::Null),
                score,
                w3_extension,
                invalidation_price,
            }
        })
        .collect();

    Ok(Json(EarlyResponse {
        venue,
        symbol,
        timeframe: tf,
        markers,
    }))
}
