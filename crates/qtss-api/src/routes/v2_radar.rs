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

use qtss_storage::resolve_account_equity_usd;

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
    pub open_exposure_usd: Option<f64>,
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
    Router::new()
        .route("/v2/radar/{market}/{period}", get(get_radar))
        .route("/v2/radar/live/{market}", get(get_radar_live))
}

/// Open-trade snapshot — the "Anlık" tab. Reads live_positions +
/// qtss_setups state for all open rows, joins against the latest mark
/// written by the tick_dispatcher so the GUI sees a fresh u-PnL without
/// needing a WS subscription of its own.
#[derive(Debug, Serialize)]
pub struct LiveTrade {
    pub setup_id: Option<uuid::Uuid>,
    pub symbol: String,
    pub timeframe: String,
    pub direction: String,
    pub mode: String,
    pub entry_price: f64,
    pub mark_price: Option<f64>,
    pub qty: f64,
    pub notional_usd: f64,
    pub sl: Option<f64>,
    pub tp: Option<f64>,
    /// Full TP ladder [tp1_price, tp2_price, tp3_price] with optional
    /// qty weights. Shape matches what the allocator writes to the
    /// setup row so the drawer can render the same structure as the
    /// Setups page.
    pub tp_ladder: serde_json::Value,
    pub u_pnl_pct: Option<f64>,
    pub u_pnl_usd: Option<f64>,
    pub opened_at: DateTime<Utc>,
    /// Leverage recorded at position open (live_positions.leverage).
    /// Dry mode paper fills default to 1 (no leverage); live Binance
    /// futures rows carry the actual isolated/cross value.
    pub leverage: i16,
    pub profile: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LiveResponse {
    pub market: String,
    pub mode: String,
    pub computed_at: DateTime<Utc>,
    pub open_count: i64,
    pub long_count: i64,
    pub short_count: i64,
    pub open_exposure_usd: f64,
    pub avg_u_pnl_pct: Option<f64>,
    pub sum_u_pnl_usd: f64,
    /// Starting capital reference (same source the aggregator uses).
    pub starting_capital_usd: f64,
    /// Unrealised mark-to-market equity = starting + sum_u_pnl_usd.
    pub current_equity_usd: f64,
    /// (capital − open_exposure) / capital, clamped to [0, 100].
    pub cash_position_pct: f64,
    /// Distinct symbol count — same breakdown as the aggregator's
    /// portfolio spread / correlation heuristic.
    pub distinct_symbols: i64,
    pub trades: Vec<LiveTrade>,
}

async fn get_radar_live(
    State(st): State<SharedState>,
    Path(market): Path<String>,
    Query(q): Query<RadarQuery>,
) -> Result<Json<LiveResponse>, ApiError> {
    let mode = q.mode.unwrap_or_else(|| "dry".to_string());
    // The venue filter mirrors the aggregator's mapping so "coin" means
    // crypto. Future markets (bist / us_equities) slot in when their
    // pipelines come online.
    let venue_class = match market.as_str() {
        "coin" => "crypto",
        "bist" => "bist",
        "nasdaq" => "us_equities",
        _ => "crypto",
    };
    let rows = sqlx::query(
        r#"SELECT
             s.id           AS setup_id,
             lp.symbol,
             s.timeframe,
             s.direction,
             lp.mode,
             lp.entry_avg,
             lp.last_mark,
             lp.qty_remaining,
             s.entry_sl,
             s.target_ref,
             s.tp_ladder,
             s.profile,
             lp.side,
             lp.leverage,
             lp.opened_at
           FROM live_positions lp
           LEFT JOIN qtss_setups s ON s.id = lp.setup_id
          WHERE lp.closed_at IS NULL
            AND lp.mode = $1
            AND (s.venue_class IS NULL OR s.venue_class = $2)
          ORDER BY lp.opened_at DESC"#,
    )
    .bind(&mode)
    .bind(venue_class)
    .fetch_all(&st.pool)
    .await?;

    let mut trades: Vec<LiveTrade> = Vec::with_capacity(rows.len());
    let mut open_exposure = 0.0f64;
    let mut long_count = 0i64;
    let mut short_count = 0i64;
    let mut pnl_pct_sum = 0.0f64;
    let mut pnl_pct_n = 0i64;
    let mut pnl_usd_sum = 0.0f64;
    for r in rows {
        let entry_avg: rust_decimal::Decimal = r.try_get("entry_avg").unwrap_or_default();
        let mark_opt: Option<rust_decimal::Decimal> = r.try_get("last_mark").ok();
        let qty: rust_decimal::Decimal = r.try_get("qty_remaining").unwrap_or_default();
        let side: String = r.try_get("side").unwrap_or_default();
        let direction: String = r
            .try_get::<Option<String>, _>("direction")
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                // Fallback when the join to qtss_setups returned NULL —
                // infer from BUY/SELL side.
                if side == "BUY" { "long".to_string() } else { "short".to_string() }
            });
        use rust_decimal::prelude::ToPrimitive;
        let entry_f = entry_avg.to_f64().unwrap_or(0.0);
        let qty_f = qty.to_f64().unwrap_or(0.0);
        let mark_f = mark_opt.and_then(|d| d.to_f64());
        let notional = entry_f * qty_f;
        open_exposure += notional;
        if direction == "long" {
            long_count += 1;
        } else {
            short_count += 1;
        }
        let (u_pct, u_usd) = match mark_f {
            Some(m) if entry_f > 0.0 => {
                let raw = ((m - entry_f) / entry_f) * 100.0;
                let signed = if direction == "long" { raw } else { -raw };
                let usd = notional * (signed / 100.0);
                pnl_pct_sum += signed;
                pnl_pct_n += 1;
                pnl_usd_sum += usd;
                (Some(signed), Some(usd))
            }
            _ => (None, None),
        };
        trades.push(LiveTrade {
            setup_id: r.try_get("setup_id").ok(),
            symbol: r.try_get("symbol").unwrap_or_default(),
            timeframe: r
                .try_get::<Option<String>, _>("timeframe")
                .ok()
                .flatten()
                .unwrap_or_default(),
            direction,
            mode: r.try_get("mode").unwrap_or_default(),
            entry_price: entry_f,
            mark_price: mark_f,
            qty: qty_f,
            notional_usd: notional,
            sl: r
                .try_get::<Option<f32>, _>("entry_sl")
                .ok()
                .flatten()
                .map(|v| v as f64),
            tp: r
                .try_get::<Option<f32>, _>("target_ref")
                .ok()
                .flatten()
                .map(|v| v as f64),
            tp_ladder: r
                .try_get::<Option<serde_json::Value>, _>("tp_ladder")
                .ok()
                .flatten()
                .unwrap_or(serde_json::Value::Array(Vec::new())),
            u_pnl_pct: u_pct,
            u_pnl_usd: u_usd,
            opened_at: r.try_get("opened_at").unwrap_or_else(|_| Utc::now()),
            leverage: r.try_get("leverage").unwrap_or(1),
            profile: r
                .try_get::<Option<String>, _>("profile")
                .ok()
                .flatten(),
        });
    }
    let avg_u_pnl_pct = if pnl_pct_n > 0 {
        Some(pnl_pct_sum / pnl_pct_n as f64)
    } else {
        None
    };

    // FAZ 25.x — single source of truth: prefer `account.equity_usd`
    // (master), fall back to the legacy radar key during migration.
    // 1000 USD baseline ensures /reports never paints the stale 1.5 M
    // ghost again. Resolver: `qtss_storage::resolve_account_equity_usd`.
    let starting_capital = resolve_account_equity_usd(
        &st.pool,
        "radar",
        "default_starting_capital_usd",
    )
    .await;
    let current_equity = starting_capital + pnl_usd_sum;
    let cash_position_pct = if starting_capital > 0.0 {
        ((starting_capital - open_exposure).max(0.0) / starting_capital * 100.0)
            .clamp(0.0, 100.0)
    } else {
        100.0
    };
    let distinct_symbols = {
        let mut set: std::collections::HashSet<String> = std::collections::HashSet::new();
        for t in &trades {
            set.insert(t.symbol.clone());
        }
        set.len() as i64
    };
    Ok(Json(LiveResponse {
        market,
        mode,
        computed_at: Utc::now(),
        open_count: trades.len() as i64,
        long_count,
        short_count,
        open_exposure_usd: open_exposure,
        avg_u_pnl_pct,
        sum_u_pnl_usd: pnl_usd_sum,
        starting_capital_usd: starting_capital,
        current_equity_usd: current_equity,
        cash_position_pct,
        distinct_symbols,
        trades,
    }))
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
        open_exposure_usd: r.try_get("open_exposure_usd").ok(),
        risk_mode: r.try_get("risk_mode").ok(),
        volatility_level: r.try_get("volatility_level").ok(),
        max_position_risk_pct: r.try_get("max_position_risk_pct").ok(),
        trades: r.try_get("trades").unwrap_or(serde_json::Value::Null),
        computed_at: r.try_get("computed_at").unwrap_or_else(|_| Utc::now()),
    }
}
