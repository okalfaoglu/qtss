//! Faz 9.8.3 — Liquidation guard events persistence.
//!
//! The risk crate produces a pure assessment; the caller (execution
//! worker) converts it into an [`InsertLiquidationEvent`] and persists
//! via [`insert`]. Keeps qtss-storage free of qtss-risk dependency
//! (CLAUDE.md #3 — layers talk in DTOs, not domain types).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

/// Minimal row payload for insertion. Field names match the table
/// columns from migration 0143.
#[derive(Debug, Clone)]
pub struct InsertLiquidationEvent {
    pub position_id: Uuid,
    /// One of `'warn' | 'high' | 'breach'` (enforced by DB constraint).
    pub severity: &'static str,
    /// One of `'none' | 'alert' | 'add_margin' | 'scale_out' | 'panic_close'`.
    pub action_taken: &'static str,
    pub mark_price: Decimal,
    pub liquidation_price: Decimal,
    pub distance_pct: Decimal,
    pub margin_ratio: Option<Decimal>,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LiquidationGuardEventRow {
    pub id: Uuid,
    pub position_id: Uuid,
    pub severity: String,
    pub action_taken: String,
    pub mark_price: Decimal,
    pub liquidation_price: Decimal,
    pub distance_pct: Decimal,
    pub margin_ratio: Option<Decimal>,
    pub details: serde_json::Value,
    pub occurred_at: DateTime<Utc>,
}

pub async fn insert(
    pool: &PgPool,
    evt: &InsertLiquidationEvent,
) -> Result<Uuid, StorageError> {
    let row: (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO liquidation_guard_events
            (position_id, severity, action_taken,
             mark_price, liquidation_price, distance_pct,
             margin_ratio, details)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id
        "#,
    )
    .bind(evt.position_id)
    .bind(evt.severity)
    .bind(evt.action_taken)
    .bind(evt.mark_price)
    .bind(evt.liquidation_price)
    .bind(evt.distance_pct)
    .bind(evt.margin_ratio)
    .bind(&evt.details)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn recent_for_position(
    pool: &PgPool,
    position_id: Uuid,
    limit: i64,
) -> Result<Vec<LiquidationGuardEventRow>, StorageError> {
    let rows = sqlx::query_as::<_, LiquidationGuardEventRow>(
        r#"
        SELECT id, position_id, severity, action_taken,
               mark_price, liquidation_price, distance_pct,
               margin_ratio, details, occurred_at
          FROM liquidation_guard_events
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
