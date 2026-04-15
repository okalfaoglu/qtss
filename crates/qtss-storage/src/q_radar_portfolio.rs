//! Q-RADAR virtual portfolio & position tracking (D/T/Q setup models).
//!
//! Manages a single virtual portfolio (~1.5M TL) with position-level
//! capital allocation, add-on buys, partial sells, and P&L tracking.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

// ── Portfolio snapshot ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct QRadarPortfolioRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub total_capital: f32,
    pub allocated_capital: f32,
    pub available_capital: f32,
    pub realized_pnl: f32,
    pub unrealized_pnl: f32,
    pub open_positions: i32,
    pub total_trades: i32,
    pub win_trades: i32,
    pub loss_trades: i32,
}

const PORTFOLIO_ID: &str = "00000000-0000-0000-0000-000000000001";

pub async fn fetch_portfolio(pool: &PgPool) -> Result<QRadarPortfolioRow, sqlx::Error> {
    sqlx::query_as::<_, QRadarPortfolioRow>(
        "SELECT * FROM q_radar_portfolio WHERE id = $1",
    )
    .bind(Uuid::parse_str(PORTFOLIO_ID).unwrap())
    .fetch_one(pool)
    .await
}

pub async fn update_portfolio_allocation(
    pool: &PgPool,
    allocated_delta: f32,
    realized_pnl_delta: f32,
    open_positions_delta: i32,
    total_trades_delta: i32,
    win_delta: i32,
    loss_delta: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE q_radar_portfolio
        SET allocated_capital = allocated_capital + $1,
            available_capital = available_capital - $1,
            realized_pnl = realized_pnl + $2,
            open_positions = open_positions + $3,
            total_trades = total_trades + $4,
            win_trades = win_trades + $5,
            loss_trades = loss_trades + $6
        WHERE id = $7
        "#,
    )
    .bind(allocated_delta)
    .bind(realized_pnl_delta)
    .bind(open_positions_delta)
    .bind(total_trades_delta)
    .bind(win_delta)
    .bind(loss_delta)
    .bind(Uuid::parse_str(PORTFOLIO_ID).unwrap())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn reset_portfolio_capital(pool: &PgPool, total: f32) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE q_radar_portfolio
        SET total_capital = $1, available_capital = $1, allocated_capital = 0,
            realized_pnl = 0, unrealized_pnl = 0, open_positions = 0,
            total_trades = 0, win_trades = 0, loss_trades = 0
        WHERE id = $2
        "#,
    )
    .bind(total)
    .bind(Uuid::parse_str(PORTFOLIO_ID).unwrap())
    .execute(pool)
    .await?;
    Ok(())
}

// ── Positions ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct QRadarPositionRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub setup_id: Uuid,
    pub symbol: String,
    pub direction: String,
    pub allocated_amount: f32,
    pub quantity: f32,
    pub avg_entry_price: f32,
    pub total_bought_qty: f32,
    pub total_sold_qty: f32,
    pub realized_pnl: f32,
    pub state: String,
    pub closed_at: Option<DateTime<Utc>>,
}

pub async fn open_position(
    pool: &PgPool,
    setup_id: Uuid,
    symbol: &str,
    direction: &str,
    allocated_amount: f32,
    quantity: f32,
    entry_price: f32,
) -> Result<Uuid, sqlx::Error> {
    let row = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO q_radar_positions
            (setup_id, symbol, direction, allocated_amount, quantity, avg_entry_price, total_bought_qty)
        VALUES ($1, $2, $3, $4, $5, $6, $5)
        RETURNING id
        "#,
    )
    .bind(setup_id)
    .bind(symbol)
    .bind(direction)
    .bind(allocated_amount)
    .bind(quantity)
    .bind(entry_price)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn fetch_open_position_for_setup(
    pool: &PgPool,
    setup_id: Uuid,
) -> Result<Option<QRadarPositionRow>, sqlx::Error> {
    sqlx::query_as::<_, QRadarPositionRow>(
        "SELECT * FROM q_radar_positions WHERE setup_id = $1 AND state = 'open'",
    )
    .bind(setup_id)
    .fetch_optional(pool)
    .await
}

pub async fn list_open_positions(pool: &PgPool) -> Result<Vec<QRadarPositionRow>, sqlx::Error> {
    sqlx::query_as::<_, QRadarPositionRow>(
        "SELECT * FROM q_radar_positions WHERE state = 'open' ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
}

pub async fn add_on_buy(
    pool: &PgPool,
    position_id: Uuid,
    add_qty: f32,
    price: f32,
) -> Result<(), sqlx::Error> {
    // Update avg_entry_price weighted by quantities.
    sqlx::query(
        r#"
        UPDATE q_radar_positions
        SET avg_entry_price = (avg_entry_price * quantity + $2 * $3) / (quantity + $2),
            quantity = quantity + $2,
            total_bought_qty = total_bought_qty + $2,
            updated_at = now()
        WHERE id = $1 AND state = 'open'
        "#,
    )
    .bind(position_id)
    .bind(add_qty)
    .bind(price)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn partial_sell(
    pool: &PgPool,
    position_id: Uuid,
    sell_qty: f32,
    _price: f32,
    pnl: f32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE q_radar_positions
        SET quantity = quantity - $2,
            total_sold_qty = total_sold_qty + $2,
            realized_pnl = realized_pnl + $3,
            updated_at = now()
        WHERE id = $1 AND state = 'open'
        "#,
    )
    .bind(position_id)
    .bind(sell_qty)
    .bind(pnl)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn close_position(
    pool: &PgPool,
    position_id: Uuid,
    final_pnl: f32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE q_radar_positions
        SET state = 'closed',
            closed_at = now(),
            quantity = 0,
            realized_pnl = realized_pnl + $2,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(position_id)
    .bind(final_pnl)
    .execute(pool)
    .await?;
    Ok(())
}

// ── Position events ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct QRadarPositionEventRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub position_id: Uuid,
    pub event_type: String,
    pub quantity: f32,
    pub price: f32,
    pub pnl: Option<f32>,
    pub notes: Option<String>,
    pub raw_meta: serde_json::Value,
}

pub async fn insert_position_event(
    pool: &PgPool,
    position_id: Uuid,
    event_type: &str,
    quantity: f32,
    price: f32,
    pnl: Option<f32>,
    notes: Option<&str>,
) -> Result<Uuid, sqlx::Error> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO q_radar_position_events (position_id, event_type, quantity, price, pnl, notes)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id
        "#,
    )
    .bind(position_id)
    .bind(event_type)
    .bind(quantity)
    .bind(price)
    .bind(pnl)
    .bind(notes)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn list_position_events(
    pool: &PgPool,
    position_id: Uuid,
) -> Result<Vec<QRadarPositionEventRow>, sqlx::Error> {
    sqlx::query_as::<_, QRadarPositionEventRow>(
        "SELECT * FROM q_radar_position_events WHERE position_id = $1 ORDER BY created_at ASC",
    )
    .bind(position_id)
    .fetch_all(pool)
    .await
}
