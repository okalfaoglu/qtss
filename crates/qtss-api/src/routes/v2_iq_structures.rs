//! `GET /v2/iq-structures/{exchange}/{symbol}/{tf}` — IQ structure
//! tracker rows for a chart panel (FAZ 25 PR-25B).
//!
//! Returns the active + last-completed/invalidated `iq_structures`
//! row(s) so the frontend can overlay current wave / state / lock
//! info on the chart. Read-only — the worker writes them; we just
//! surface them.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Serialize)]
pub struct IqStructure {
    pub id: uuid::Uuid,
    pub slot: i16,
    pub direction: i16,
    pub state: String,
    pub current_wave: String,
    pub current_stage: String,
    pub structure_anchors: Value,
    pub seed_hash: String,
    pub started_at: DateTime<Utc>,
    pub last_advanced_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub invalidated_at: Option<DateTime<Utc>>,
    pub invalidation_reason: Option<String>,
    pub raw_meta: Value,
}

#[derive(Debug, Serialize)]
pub struct IqLock {
    pub locked_at: DateTime<Utc>,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct IqStructuresResponse {
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub timeframe: String,
    pub structures: Vec<IqStructure>,
    pub lock: Option<IqLock>,
}

#[derive(Debug, Deserialize)]
pub struct Q {
    pub segment: Option<String>,
    /// Cap on returned structures (default 10, applied AFTER active
    /// rows are surfaced first).
    pub limit: Option<i64>,
}

pub fn v2_iq_structures_router() -> Router<SharedState> {
    Router::new().route(
        "/v2/iq-structures/{exchange}/{symbol}/{tf}",
        get(get_iq_structures),
    )
}

async fn get_iq_structures(
    State(state): State<SharedState>,
    Path((exchange, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<Q>,
) -> Result<Json<IqStructuresResponse>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(10).clamp(1, 200);
    let pool = &state.pool;

    let rows = sqlx::query(
        r#"SELECT id, slot, direction, state, current_wave, current_stage,
                  structure_anchors, seed_hash,
                  started_at, last_advanced_at, completed_at,
                  invalidated_at, invalidation_reason, raw_meta
             FROM iq_structures
            WHERE exchange = $1 AND segment = $2
              AND symbol = $3 AND timeframe = $4
            ORDER BY
              CASE state
                WHEN 'tracking'    THEN 0
                WHEN 'candidate'   THEN 1
                WHEN 'completed'   THEN 2
                WHEN 'invalidated' THEN 3
                ELSE 4
              END,
              last_advanced_at DESC
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
            format!("iq_structures query failed: {e}"),
        )
    })?;

    let structures: Vec<IqStructure> = rows
        .into_iter()
        .map(|r| IqStructure {
            id: r.try_get("id").unwrap_or_default(),
            slot: r.try_get("slot").unwrap_or(0),
            direction: r.try_get("direction").unwrap_or(0),
            state: r.try_get("state").unwrap_or_default(),
            current_wave: r.try_get("current_wave").unwrap_or_default(),
            current_stage: r.try_get("current_stage").unwrap_or_default(),
            structure_anchors: r.try_get("structure_anchors").unwrap_or(Value::Null),
            seed_hash: r.try_get("seed_hash").unwrap_or_default(),
            started_at: r.try_get("started_at").unwrap_or_else(|_| Utc::now()),
            last_advanced_at: r
                .try_get("last_advanced_at")
                .unwrap_or_else(|_| Utc::now()),
            completed_at: r.try_get("completed_at").ok(),
            invalidated_at: r.try_get("invalidated_at").ok(),
            invalidation_reason: r.try_get("invalidation_reason").ok(),
            raw_meta: r.try_get("raw_meta").unwrap_or(Value::Null),
        })
        .collect();

    let lock_row = sqlx::query(
        r#"SELECT locked_at, reason FROM iq_symbol_locks
            WHERE exchange = $1 AND segment = $2 AND symbol = $3"#,
    )
    .bind(&exchange)
    .bind(&segment)
    .bind(&symbol)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("iq_symbol_locks query failed: {e}"),
        )
    })?;
    let lock = lock_row.map(|r| IqLock {
        locked_at: r.try_get("locked_at").unwrap_or_else(|_| Utc::now()),
        reason: r.try_get("reason").ok(),
    });

    Ok(Json(IqStructuresResponse {
        exchange,
        segment,
        symbol,
        timeframe: tf,
        structures,
        lock,
    }))
}
