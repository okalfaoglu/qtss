//! Faz 9.8.13 — `live_positions` persistence.
//!
//! The risk crate's `LivePositionStore` lives in memory. For the tick
//! dispatcher to work across worker restarts, and for the GUI to show
//! an honest list of open positions, every dispatched order must also
//! produce a row here. This module keeps qtss-storage free of
//! qtss-risk types (CLAUDE.md #3) — callers shape primitives + JSON.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone)]
pub struct InsertLivePosition {
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub setup_id: Option<Uuid>,
    pub mode: &'static str, // 'dry' | 'live'
    pub exchange: String,
    pub segment: String, // 'spot' | 'futures' | 'margin' | 'options'
    pub symbol: String,
    pub side: &'static str, // 'BUY' | 'SELL'
    pub leverage: i16,
    pub entry_avg: Decimal,
    pub qty_filled: Decimal,
    pub qty_remaining: Decimal,
    pub current_sl: Option<Decimal>,
    pub tp_ladder: JsonValue,
    pub liquidation_price: Option<Decimal>,
    pub maint_margin_ratio: Option<Decimal>,
    pub last_mark: Option<Decimal>,
    pub metadata: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LivePositionRow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub setup_id: Option<Uuid>,
    pub mode: String,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub side: String,
    pub leverage: i16,
    pub entry_avg: Decimal,
    pub qty_filled: Decimal,
    pub qty_remaining: Decimal,
    pub current_sl: Option<Decimal>,
    pub tp_ladder: JsonValue,
    pub liquidation_price: Option<Decimal>,
    pub maint_margin_ratio: Option<Decimal>,
    pub funding_rate_next: Option<Decimal>,
    pub last_mark: Option<Decimal>,
    pub last_tick_at: Option<DateTime<Utc>>,
    pub opened_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub close_reason: Option<String>,
    pub realized_pnl_quote: Option<Decimal>,
    pub metadata: JsonValue,
}

pub async fn insert(pool: &PgPool, p: &InsertLivePosition) -> Result<Uuid, StorageError> {
    let row: (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO live_positions (
            org_id, user_id, setup_id, mode,
            exchange, segment, symbol, side, leverage,
            entry_avg, qty_filled, qty_remaining,
            current_sl, tp_ladder,
            liquidation_price, maint_margin_ratio,
            last_mark, metadata
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)
        RETURNING id
        "#,
    )
    .bind(p.org_id)
    .bind(p.user_id)
    .bind(p.setup_id)
    .bind(p.mode)
    .bind(&p.exchange)
    .bind(&p.segment)
    .bind(&p.symbol)
    .bind(p.side)
    .bind(p.leverage)
    .bind(p.entry_avg)
    .bind(p.qty_filled)
    .bind(p.qty_remaining)
    .bind(p.current_sl)
    .bind(&p.tp_ladder)
    .bind(p.liquidation_price)
    .bind(p.maint_margin_ratio)
    .bind(p.last_mark)
    .bind(&p.metadata)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn list_open(pool: &PgPool, mode: Option<&str>) -> Result<Vec<LivePositionRow>, StorageError> {
    let rows = sqlx::query_as::<_, LivePositionRow>(
        r#"
        SELECT id, org_id, user_id, setup_id, mode, exchange, segment,
               symbol, side, leverage, entry_avg, qty_filled, qty_remaining,
               current_sl, tp_ladder, liquidation_price, maint_margin_ratio,
               funding_rate_next, last_mark, last_tick_at, opened_at,
               closed_at, close_reason, realized_pnl_quote, metadata
          FROM live_positions
         WHERE closed_at IS NULL
           AND ($1::text IS NULL OR mode = $1)
         ORDER BY opened_at DESC
        "#,
    )
    .bind(mode)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn update_mark(
    pool: &PgPool,
    id: Uuid,
    mark: Decimal,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE live_positions
              SET last_mark = $2, last_tick_at = now(), updated_at = now()
            WHERE id = $1 AND closed_at IS NULL"#,
    )
    .bind(id)
    .bind(mark)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn close(
    pool: &PgPool,
    id: Uuid,
    reason: &str,
    realized_pnl_quote: Option<Decimal>,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"UPDATE live_positions
              SET closed_at = now(), close_reason = $2,
                  realized_pnl_quote = $3, updated_at = now()
            WHERE id = $1 AND closed_at IS NULL"#,
    )
    .bind(id)
    .bind(reason)
    .bind(realized_pnl_quote)
    .execute(pool)
    .await?;
    Ok(())
}
