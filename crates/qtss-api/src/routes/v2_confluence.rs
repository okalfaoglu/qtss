//! `GET /v2/confluence/{venue}/{symbol}/{tf}` — recent per-tick
//! confluence scores for the symbol/TF. Returns the last N snapshots
//! in reverse-chronological order. The chart widget reads the head of
//! the series for the current reading and optionally plots the trail.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct ConfluenceQuery {
    pub segment: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ConfluenceSnap {
    pub computed_at: DateTime<Utc>,
    pub bull_score: f64,
    pub bear_score: f64,
    pub net_score: f64,
    pub confidence: f64,
    pub verdict: String,
    pub contributors: serde_json::Value,
    pub regime: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ConfluenceResp {
    pub venue: String,
    pub symbol: String,
    pub timeframe: String,
    pub snapshots: Vec<ConfluenceSnap>,
}

pub fn v2_confluence_router() -> Router<SharedState> {
    Router::new().route(
        "/v2/confluence/{venue}/{symbol}/{tf}",
        get(get_confluence),
    )
}

async fn get_confluence(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<ConfluenceQuery>,
) -> Result<Json<ConfluenceResp>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(60).clamp(1, 1000);
    let rows = sqlx::query(
        r#"SELECT computed_at, bull_score, bear_score, net_score, confidence,
                  verdict, contributors, regime
             FROM confluence_snapshots
            WHERE exchange = $1 AND segment = $2 AND symbol = $3 AND timeframe = $4
            ORDER BY computed_at DESC
            LIMIT $5"#,
    )
    .bind(&venue)
    .bind(&segment)
    .bind(&symbol)
    .bind(&tf)
    .bind(limit)
    .fetch_all(&st.pool)
    .await?;
    let snapshots: Vec<ConfluenceSnap> = rows
        .into_iter()
        .map(|r| ConfluenceSnap {
            computed_at: r.get("computed_at"),
            bull_score: r.try_get("bull_score").unwrap_or(0.0),
            bear_score: r.try_get("bear_score").unwrap_or(0.0),
            net_score: r.try_get("net_score").unwrap_or(0.0),
            confidence: r.try_get("confidence").unwrap_or(0.0),
            verdict: r.try_get("verdict").unwrap_or_default(),
            contributors: r.try_get("contributors").unwrap_or(serde_json::Value::Null),
            regime: r.try_get("regime").ok(),
        })
        .collect();
    Ok(Json(ConfluenceResp {
        venue,
        symbol,
        timeframe: tf,
        snapshots,
    }))
}
