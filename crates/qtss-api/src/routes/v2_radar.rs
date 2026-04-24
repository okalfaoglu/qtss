//! `GET /v2/radar/{market}/{period}` — QTSS RADAR performance snapshot.
//! Returns the active (or latest) report row for the requested
//! market × period, optionally filtered by mode (live / dry / backtest).
//! GUI reads this endpoint to populate the /v2/reports page; PDF
//! export (future) also builds from this shape.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct RadarQuery {
    pub mode: Option<String>,     // 'live' | 'dry' | 'backtest'
    pub history: Option<bool>,    // if true, return the last 12 rows
}

#[derive(Debug, Serialize)]
pub struct RadarReport {
    pub market: String,
    pub period: String,
    pub mode: String,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub finalised: bool,
    pub closed_count: i32,
    pub win_count: i32,
    pub loss_count: i32,
    pub win_rate: Option<f64>,
    pub total_notional_usd: Option<f64>,
    pub total_pnl_usd: Option<f64>,
    pub avg_return_pct: Option<f64>,
    pub compound_return_pct: Option<f64>,
    pub avg_allocation_usd: Option<f64>,
    pub avg_holding_bars: Option<f64>,
    pub max_drawdown_pct: Option<f64>,
    pub starting_capital_usd: Option<f64>,
    pub ending_capital_usd: Option<f64>,
    pub cash_position_pct: Option<f64>,
    pub risk_mode: Option<String>,
    pub volatility_level: Option<String>,
    pub max_position_risk_pct: Option<f64>,
    pub trades: serde_json::Value,
    pub computed_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct RadarResponse {
    pub market: String,
    pub period: String,
    pub latest: Option<RadarReport>,
    pub history: Vec<RadarReport>,
}

pub fn v2_radar_router() -> Router<SharedState> {
    Router::new().route("/v2/radar/{market}/{period}", get(get_radar))
}

async fn get_radar(
    State(st): State<SharedState>,
    Path((market, period)): Path<(String, String)>,
    Query(q): Query<RadarQuery>,
) -> Result<Json<RadarResponse>, ApiError> {
    let mode = q.mode.unwrap_or_else(|| "dry".to_string());
    let want_history = q.history.unwrap_or(false);

    // Latest (active) row.
    let row = sqlx::query(
        r#"SELECT * FROM radar_reports
            WHERE market=$1 AND period=$2 AND mode=$3
            ORDER BY period_end DESC
            LIMIT 1"#,
    )
    .bind(&market)
    .bind(&period)
    .bind(&mode)
    .fetch_optional(&st.pool)
    .await?;
    let latest = row.map(row_to_report);

    let history = if want_history {
        let rows = sqlx::query(
            r#"SELECT * FROM radar_reports
                WHERE market=$1 AND period=$2 AND mode=$3
                ORDER BY period_end DESC
                LIMIT 12"#,
        )
        .bind(&market)
        .bind(&period)
        .bind(&mode)
        .fetch_all(&st.pool)
        .await?;
        rows.into_iter().map(row_to_report).collect()
    } else {
        Vec::new()
    };

    Ok(Json(RadarResponse {
        market,
        period,
        latest,
        history,
    }))
}

fn row_to_report(r: sqlx::postgres::PgRow) -> RadarReport {
    RadarReport {
        market: r.try_get("market").unwrap_or_default(),
        period: r.try_get("period").unwrap_or_default(),
        mode: r.try_get("mode").unwrap_or_default(),
        period_start: r.try_get("period_start").unwrap_or_else(|_| Utc::now()),
        period_end: r.try_get("period_end").unwrap_or_else(|_| Utc::now()),
        finalised: r.try_get("finalised").unwrap_or(false),
        closed_count: r.try_get("closed_count").unwrap_or(0),
        win_count: r.try_get("win_count").unwrap_or(0),
        loss_count: r.try_get("loss_count").unwrap_or(0),
        win_rate: r.try_get("win_rate").ok(),
        total_notional_usd: r.try_get("total_notional_usd").ok(),
        total_pnl_usd: r.try_get("total_pnl_usd").ok(),
        avg_return_pct: r.try_get("avg_return_pct").ok(),
        compound_return_pct: r.try_get("compound_return_pct").ok(),
        avg_allocation_usd: r.try_get("avg_allocation_usd").ok(),
        avg_holding_bars: r.try_get("avg_holding_bars").ok(),
        max_drawdown_pct: r.try_get("max_drawdown_pct").ok(),
        starting_capital_usd: r.try_get("starting_capital_usd").ok(),
        ending_capital_usd: r.try_get("ending_capital_usd").ok(),
        cash_position_pct: r.try_get("cash_position_pct").ok(),
        risk_mode: r.try_get("risk_mode").ok(),
        volatility_level: r.try_get("volatility_level").ok(),
        max_position_risk_pct: r.try_get("max_position_risk_pct").ok(),
        trades: r.try_get("trades").unwrap_or(serde_json::Value::Null),
        computed_at: r.try_get("computed_at").unwrap_or_else(|_| Utc::now()),
    }
}
