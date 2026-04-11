//! Q-RADAR virtual portfolio API endpoints.
//!
//! - `GET /v2/q-radar/portfolio` — current portfolio snapshot
//! - `GET /v2/q-radar/positions` — open positions
//! - `GET /v2/q-radar/positions/{id}/events` — position event history

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use qtss_storage::q_radar_portfolio;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Serialize)]
pub struct PortfolioView {
    pub generated_at: DateTime<Utc>,
    pub total_capital: f32,
    pub allocated_capital: f32,
    pub available_capital: f32,
    pub allocated_pct: f32,
    pub realized_pnl: f32,
    pub unrealized_pnl: f32,
    pub total_pnl: f32,
    pub open_positions: i32,
    pub total_trades: i32,
    pub win_trades: i32,
    pub loss_trades: i32,
    pub win_rate: f32,
}

#[derive(Debug, Serialize)]
pub struct PositionView {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
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

#[derive(Debug, Serialize)]
pub struct PositionEventView {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub event_type: String,
    pub quantity: f32,
    pub price: f32,
    pub pnl: Option<f32>,
    pub notes: Option<String>,
}

pub fn v2_q_radar_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/q-radar/portfolio", get(get_portfolio))
        .route("/v2/q-radar/positions", get(get_positions))
        .route("/v2/q-radar/positions/{id}/events", get(get_position_events))
}

async fn get_portfolio(
    State(st): State<SharedState>,
) -> Result<Json<PortfolioView>, ApiError> {
    let p = q_radar_portfolio::fetch_portfolio(&st.pool).await?;
    let total_pnl = p.realized_pnl + p.unrealized_pnl;
    let allocated_pct = if p.total_capital > 0.0 {
        (p.allocated_capital / p.total_capital) * 100.0
    } else {
        0.0
    };
    let win_rate = if p.total_trades > 0 {
        (p.win_trades as f32 / p.total_trades as f32) * 100.0
    } else {
        0.0
    };
    Ok(Json(PortfolioView {
        generated_at: Utc::now(),
        total_capital: p.total_capital,
        allocated_capital: p.allocated_capital,
        available_capital: p.available_capital,
        allocated_pct,
        realized_pnl: p.realized_pnl,
        unrealized_pnl: p.unrealized_pnl,
        total_pnl,
        open_positions: p.open_positions,
        total_trades: p.total_trades,
        win_trades: p.win_trades,
        loss_trades: p.loss_trades,
        win_rate,
    }))
}

async fn get_positions(
    State(st): State<SharedState>,
) -> Result<Json<Vec<PositionView>>, ApiError> {
    let rows = q_radar_portfolio::list_open_positions(&st.pool).await?;
    let views: Vec<PositionView> = rows.into_iter().map(|r| PositionView {
        id: r.id,
        created_at: r.created_at,
        setup_id: r.setup_id,
        symbol: r.symbol,
        direction: r.direction,
        allocated_amount: r.allocated_amount,
        quantity: r.quantity,
        avg_entry_price: r.avg_entry_price,
        total_bought_qty: r.total_bought_qty,
        total_sold_qty: r.total_sold_qty,
        realized_pnl: r.realized_pnl,
        state: r.state,
        closed_at: r.closed_at,
    }).collect();
    Ok(Json(views))
}

async fn get_position_events(
    State(st): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<PositionEventView>>, ApiError> {
    let rows = q_radar_portfolio::list_position_events(&st.pool, id).await?;
    let views: Vec<PositionEventView> = rows.into_iter().map(|r| PositionEventView {
        id: r.id,
        created_at: r.created_at,
        event_type: r.event_type,
        quantity: r.quantity,
        price: r.price,
        pnl: r.pnl,
        notes: r.notes,
    }).collect();
    Ok(Json(views))
}
