//! `GET /v2/iq-setups/{exchange}/{symbol}/{tf}` — IQ-D / IQ-T setups
//! for the chart panel (FAZ 25 PR-25C/D).
//!
//! Returns the active and recently-closed iq_d / iq_t rows from
//! `qtss_setups` so the frontend can paint entry / SL / TP bands on
//! the IQ Chart and surface the parent → child link in the sidebar.
//! Read-only — the worker writes them, we just query.

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
pub struct IqSetup {
    pub id: uuid::Uuid,
    pub profile: String,                       // "iq_d" or "iq_t"
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub direction: String,                     // "long" | "short"
    pub state: String,
    pub entry_price: Option<f32>,
    pub entry_sl: Option<f32>,
    pub target_ref: Option<f32>,
    pub created_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub parent_setup_id: Option<uuid::Uuid>,
    pub raw_meta: Value,
}

#[derive(Debug, Serialize)]
pub struct IqSetupsResponse {
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub timeframe: String,
    pub setups: Vec<IqSetup>,
}

#[derive(Debug, Deserialize)]
pub struct Q {
    pub segment: Option<String>,
    /// Cap on returned rows. Active states surface first.
    pub limit: Option<i64>,
    /// Include closed setups (default false). Useful for backtest
    /// review; the live IQ Chart paints only active rows.
    pub include_closed: Option<bool>,
}

pub fn v2_iq_setups_router() -> Router<SharedState> {
    Router::new().route(
        "/v2/iq-setups/{exchange}/{symbol}/{tf}",
        get(get_iq_setups),
    )
}

async fn get_iq_setups(
    State(state): State<SharedState>,
    Path((exchange, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<Q>,
) -> Result<Json<IqSetupsResponse>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(20).clamp(1, 200);
    let include_closed = q.include_closed.unwrap_or(false);
    let pool = &state.pool;

    let rows = sqlx::query(
        r#"SELECT id, profile, exchange, symbol, timeframe, direction, state,
                  entry_price, entry_sl, target_ref,
                  created_at, closed_at, parent_setup_id, raw_meta
             FROM qtss_setups
            WHERE exchange = $1 AND symbol = $2 AND timeframe = $3
              AND profile IN ('iq_d', 'iq_t')
              AND ($4 OR state IN ('flat','armed','active'))
            ORDER BY
              CASE state
                WHEN 'active' THEN 0
                WHEN 'armed'  THEN 1
                WHEN 'flat'   THEN 2
                ELSE 3
              END,
              created_at DESC
            LIMIT $5"#,
    )
    .bind(&exchange)
    .bind(&symbol)
    .bind(&tf)
    .bind(include_closed)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("iq_setups query failed: {e}"),
        )
    })?;

    let setups: Vec<IqSetup> = rows
        .into_iter()
        .map(|r| IqSetup {
            id: r.try_get("id").unwrap_or_default(),
            profile: r.try_get("profile").unwrap_or_default(),
            exchange: r.try_get("exchange").unwrap_or_default(),
            symbol: r.try_get("symbol").unwrap_or_default(),
            timeframe: r.try_get("timeframe").unwrap_or_default(),
            direction: r.try_get("direction").unwrap_or_default(),
            state: r.try_get("state").unwrap_or_default(),
            entry_price: r.try_get("entry_price").ok(),
            entry_sl: r.try_get("entry_sl").ok(),
            target_ref: r.try_get("target_ref").ok(),
            created_at: r.try_get("created_at").unwrap_or_else(|_| Utc::now()),
            closed_at: r.try_get("closed_at").ok(),
            parent_setup_id: r.try_get("parent_setup_id").ok(),
            raw_meta: r.try_get("raw_meta").unwrap_or(Value::Null),
        })
        .collect();

    Ok(Json(IqSetupsResponse {
        exchange,
        segment,
        symbol,
        timeframe: tf,
        setups,
    }))
}
