//! `GET /v2/positions/:id/events` — Faz 9.8.23.
//!
//! Combined feed of `position_scale_events` + `liquidation_guard_events`
//! for a single live position. Lets the GUI render a timeline of
//! pyramid / scale-out / ratchet / partial-tp / liq-warn events.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    /// Cap (default 200, max 1000).
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PositionEvent {
    Scale {
        at: DateTime<Utc>,
        event_kind: String,
        price: Decimal,
        qty_delta: Decimal,
        reason: Option<String>,
    },
    Liquidation {
        at: DateTime<Utc>,
        severity: String,
        action_taken: String,
        mark_price: Decimal,
        liquidation_price: Decimal,
        distance_pct: Decimal,
    },
}

pub fn v2_position_events_router() -> Router<SharedState> {
    Router::new().route("/v2/positions/{id}/events", get(list))
}

async fn list(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
    Query(q): Query<EventsQuery>,
) -> Result<Json<Vec<PositionEvent>>, ApiError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 1_000);
    let mut events = fetch_scale(&st.pool, id, limit).await?;
    events.extend(fetch_liquidation(&st.pool, id, limit).await?);
    // Sort descending by timestamp.
    events.sort_by(|a, b| occurred_at(b).cmp(&occurred_at(a)));
    events.truncate(limit as usize);
    Ok(Json(events))
}

fn occurred_at(e: &PositionEvent) -> DateTime<Utc> {
    match e {
        PositionEvent::Scale { at, .. } => *at,
        PositionEvent::Liquidation { at, .. } => *at,
    }
}

async fn fetch_scale(
    pool: &sqlx::PgPool,
    id: Uuid,
    limit: i64,
) -> Result<Vec<PositionEvent>, ApiError> {
    let rows: Vec<(DateTime<Utc>, String, Decimal, Decimal, Option<String>)> = sqlx::query_as(
        r#"SELECT occurred_at, event_kind, price, qty_delta, reason
             FROM position_scale_events
            WHERE position_id = $1
            ORDER BY occurred_at DESC
            LIMIT $2"#,
    )
    .bind(id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::internal(format!("position_scale_events: {e}")))?;
    Ok(rows
        .into_iter()
        .map(|(at, event_kind, price, qty_delta, reason)| PositionEvent::Scale {
            at,
            event_kind,
            price,
            qty_delta,
            reason,
        })
        .collect())
}

async fn fetch_liquidation(
    pool: &sqlx::PgPool,
    id: Uuid,
    limit: i64,
) -> Result<Vec<PositionEvent>, ApiError> {
    let rows: Vec<(
        DateTime<Utc>,
        String,
        String,
        Decimal,
        Decimal,
        Decimal,
    )> = sqlx::query_as(
        r#"SELECT occurred_at, severity, action_taken, mark_price, liquidation_price, distance_pct
             FROM liquidation_guard_events
            WHERE position_id = $1
            ORDER BY occurred_at DESC
            LIMIT $2"#,
    )
    .bind(id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::internal(format!("liquidation_guard_events: {e}")))?;
    Ok(rows
        .into_iter()
        .map(
            |(at, severity, action_taken, mark_price, liquidation_price, distance_pct)| {
                PositionEvent::Liquidation {
                    at,
                    severity,
                    action_taken,
                    mark_price,
                    liquidation_price,
                    distance_pct,
                }
            },
        )
        .collect())
}
