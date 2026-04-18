//! Faz 9.8.14 — `position_scale_events` persistence.
//!
//! The risk crate's scale manager + ratchet emit pure `ScaleDecision`
//! values. The execution worker translates them into
//! [`InsertScaleEvent`] rows and persists them here. CLAUDE.md #3 —
//! storage stays DTO-shaped, no qtss-risk types leak in.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone)]
pub struct InsertScaleEvent {
    pub position_id: Uuid,
    /// 'scale_in' | 'scale_out' | 'add_on_dip' | 'partial_tp' | 'ratchet_sl'
    pub event_kind: &'static str,
    pub price: Decimal,
    pub qty_delta: Decimal,
    pub qty_after: Decimal,
    pub entry_avg_after: Decimal,
    pub realized_pnl_quote: Option<Decimal>,
    pub reason: Option<String>,
    pub metadata: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PositionScaleEventRow {
    pub id: Uuid,
    pub position_id: Uuid,
    pub event_kind: String,
    pub price: Decimal,
    pub qty_delta: Decimal,
    pub qty_after: Decimal,
    pub entry_avg_after: Decimal,
    pub realized_pnl_quote: Option<Decimal>,
    pub reason: Option<String>,
    pub metadata: JsonValue,
    pub occurred_at: DateTime<Utc>,
}

pub async fn insert(pool: &PgPool, e: &InsertScaleEvent) -> Result<Uuid, StorageError> {
    let row: (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO position_scale_events
            (position_id, event_kind, price, qty_delta, qty_after,
             entry_avg_after, realized_pnl_quote, reason, metadata)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        RETURNING id
        "#,
    )
    .bind(e.position_id)
    .bind(e.event_kind)
    .bind(e.price)
    .bind(e.qty_delta)
    .bind(e.qty_after)
    .bind(e.entry_avg_after)
    .bind(e.realized_pnl_quote)
    .bind(&e.reason)
    .bind(&e.metadata)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn recent_for_position(
    pool: &PgPool,
    position_id: Uuid,
    limit: i64,
) -> Result<Vec<PositionScaleEventRow>, StorageError> {
    let rows = sqlx::query_as::<_, PositionScaleEventRow>(
        r#"
        SELECT id, position_id, event_kind, price, qty_delta, qty_after,
               entry_avg_after, realized_pnl_quote, reason, metadata, occurred_at
          FROM position_scale_events
         WHERE position_id = $1
         ORDER BY occurred_at DESC
         LIMIT $2
        "#,
    )
    .bind(position_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
