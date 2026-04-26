//! `GET /v2/major-dip/{venue}/{symbol}/{tf}` — Major Dip composite
//! score + 8-component breakdown for a single series. FAZ 25.3.D.
//!
//! Returns the latest `major_dip_candidates` row (highest score
//! retained per candidate_bar by the worker UPSERT). Frontend reads
//! this to:
//!   1. Render a horizontal band at candidate_price marked
//!      "Major Dip · score 0.62 (high)"
//!   2. Pop a radar/spider chart of per-component scores so the
//!      operator sees WHICH channels lit up (vs which are stubs)
//!
//! Read-only; the qtss-worker `major_dip_candidate_loop` writes.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct MajorDipQuery {
    pub segment: Option<String>,
    /// Optional `limit` — when set, return the latest N rows so the
    /// frontend can plot the dip-score history. Defaults to 1.
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct MajorDipRow {
    pub candidate_bar: i64,
    pub candidate_time: DateTime<Utc>,
    pub candidate_price: f64,
    pub score: f64,
    pub components: Value,
    pub verdict: String,
}

#[derive(Debug, Serialize)]
pub struct MajorDipResponse {
    pub venue: String,
    pub segment: String,
    pub symbol: String,
    pub timeframe: String,
    pub rows: Vec<MajorDipRow>,
}

pub fn v2_major_dip_router() -> Router<SharedState> {
    Router::new().route("/v2/major-dip/{venue}/{symbol}/{tf}", get(get_major_dip))
}

async fn get_major_dip(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<MajorDipQuery>,
) -> Result<Json<MajorDipResponse>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(1).clamp(1, 200);

    let rows = sqlx::query(
        r#"SELECT candidate_bar, candidate_time, candidate_price,
                  score, components, verdict
             FROM major_dip_candidates
            WHERE exchange = $1 AND segment = $2
              AND symbol = $3 AND timeframe = $4
            ORDER BY candidate_time DESC
            LIMIT $5"#,
    )
    .bind(&venue)
    .bind(&segment)
    .bind(&symbol)
    .bind(&tf)
    .bind(limit)
    .fetch_all(&st.pool)
    .await?;

    use rust_decimal::prelude::ToPrimitive;
    let out: Vec<MajorDipRow> = rows
        .into_iter()
        .map(|r| {
            let price: rust_decimal::Decimal = r.try_get("candidate_price").unwrap_or_default();
            MajorDipRow {
                candidate_bar: r.try_get("candidate_bar").unwrap_or(0),
                candidate_time: r.try_get("candidate_time").unwrap_or_else(|_| Utc::now()),
                candidate_price: price.to_f64().unwrap_or(0.0),
                score: r.try_get("score").unwrap_or(0.0),
                components: r.try_get("components").unwrap_or(Value::Null),
                verdict: r.try_get("verdict").unwrap_or_default(),
            }
        })
        .collect();

    Ok(Json(MajorDipResponse {
        venue,
        segment,
        symbol,
        timeframe: tf,
        rows: out,
    }))
}
