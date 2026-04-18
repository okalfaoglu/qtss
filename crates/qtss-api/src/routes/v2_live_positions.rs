//! `GET /v2/live-positions` — Faz 9.8.21.
//!
//! Surfaces the rows written by `execution_bridge` + `tick_dispatcher` so
//! the GUI can finally show open paper/live positions, live marks, and
//! realized PnL on closed ones. Read-only; mutations happen through the
//! worker path (broker gateway / tick outcomes), never through the API.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use qtss_storage::{list_open_live_positions, LivePositionRow};

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct LivePositionsQuery {
    /// Filter by execution mode: "dry" | "live". Omit for all.
    pub mode: Option<String>,
    /// Include closed positions too (default: only open).
    #[serde(default)]
    pub include_closed: bool,
    /// Cap on rows (default 100, max 500).
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct LivePositionView {
    pub id: Uuid,
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
    pub liquidation_price: Option<Decimal>,
    pub last_mark: Option<Decimal>,
    pub opened_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub close_reason: Option<String>,
    pub realized_pnl_quote: Option<Decimal>,
    /// Unrealized mark-to-market PnL for open positions. NULL if no mark yet.
    pub unrealized_pnl_quote: Option<Decimal>,
    pub tp_ladder: serde_json::Value,
}

pub fn v2_live_positions_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/live-positions", get(list))
        .route("/v2/live-positions/{id}", get(detail))
}

async fn list(
    State(st): State<SharedState>,
    Query(q): Query<LivePositionsQuery>,
) -> Result<Json<Vec<LivePositionView>>, ApiError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let mode_filter = q.mode.as_deref();

    let rows = if q.include_closed {
        fetch_any(&st.pool, mode_filter, limit).await?
    } else {
        list_open_live_positions(&st.pool, mode_filter)
            .await
            .map_err(|e| ApiError::internal(format!("live_positions: {e}")))?
    };

    let views: Vec<LivePositionView> = rows.into_iter().take(limit as usize).map(to_view).collect();
    Ok(Json(views))
}

async fn detail(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<LivePositionView>, ApiError> {
    let row = fetch_by_id(&st.pool, id)
        .await
        .map_err(|e| ApiError::internal(format!("live_positions: {e}")))?
        .ok_or_else(|| ApiError::not_found("live position not found"))?;
    Ok(Json(to_view(row)))
}

fn to_view(row: LivePositionRow) -> LivePositionView {
    let unrealized = unrealized_pnl(&row);
    LivePositionView {
        id: row.id,
        setup_id: row.setup_id,
        mode: row.mode,
        exchange: row.exchange,
        segment: row.segment,
        symbol: row.symbol,
        side: row.side,
        leverage: row.leverage,
        entry_avg: row.entry_avg,
        qty_filled: row.qty_filled,
        qty_remaining: row.qty_remaining,
        current_sl: row.current_sl,
        liquidation_price: row.liquidation_price,
        last_mark: row.last_mark,
        opened_at: row.opened_at,
        closed_at: row.closed_at,
        close_reason: row.close_reason,
        realized_pnl_quote: row.realized_pnl_quote,
        unrealized_pnl_quote: unrealized,
        tp_ladder: row.tp_ladder,
    }
}

/// Mark-to-market unrealized PnL for still-open positions. Closed rows
/// return `None` (use `realized_pnl_quote` instead).
fn unrealized_pnl(row: &LivePositionRow) -> Option<Decimal> {
    if row.closed_at.is_some() {
        return None;
    }
    let mark = row.last_mark?;
    let qty = row.qty_remaining;
    if qty <= Decimal::ZERO {
        return None;
    }
    let gross = match row.side.as_str() {
        "BUY" | "buy" => (mark - row.entry_avg) * qty,
        "SELL" | "sell" => (row.entry_avg - mark) * qty,
        _ => return None,
    };
    Some(gross)
}

async fn fetch_any(
    pool: &sqlx::PgPool,
    mode: Option<&str>,
    limit: i64,
) -> Result<Vec<LivePositionRow>, ApiError> {
    sqlx::query_as::<_, LivePositionRow>(
        r#"
        SELECT id, org_id, user_id, setup_id, mode, exchange, segment,
               symbol, side, leverage, entry_avg, qty_filled, qty_remaining,
               current_sl, tp_ladder, liquidation_price, maint_margin_ratio,
               funding_rate_next, last_mark, last_tick_at, opened_at,
               closed_at, close_reason, realized_pnl_quote, metadata
          FROM live_positions
         WHERE ($1::text IS NULL OR mode = $1)
         ORDER BY COALESCE(closed_at, opened_at) DESC
         LIMIT $2
        "#,
    )
    .bind(mode)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::internal(format!("live_positions fetch_any: {e}")))
}

async fn fetch_by_id(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> Result<Option<LivePositionRow>, sqlx::Error> {
    sqlx::query_as::<_, LivePositionRow>(
        r#"
        SELECT id, org_id, user_id, setup_id, mode, exchange, segment,
               symbol, side, leverage, entry_avg, qty_filled, qty_remaining,
               current_sl, tp_ladder, liquidation_price, maint_margin_ratio,
               funding_rate_next, last_mark, last_tick_at, opened_at,
               closed_at, close_reason, realized_pnl_quote, metadata
          FROM live_positions
         WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}
