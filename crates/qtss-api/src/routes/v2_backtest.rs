//! `GET /v2/backtest/*` — Faz 9C.
//!
//! Read-only feed for the backtest dispatcher output (setups rows with
//! `mode='backtest'`). The dispatcher in
//! `crates/qtss-worker/src/v2_backtest_setup_loop.rs` arms these from
//! historical detections; once the watcher closes them, `pnl_pct` and
//! `closed_at` are populated and this endpoint aggregates them into an
//! equity curve + per-formation breakdown for the `/backtest` GUI page.
//!
//! No writes — the dispatcher and watcher own lifecycle. GUI only reads.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::state::SharedState;

pub fn v2_backtest_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/backtest/summary", get(get_summary))
        .route("/v2/backtest/setups", get(get_setups))
}

#[derive(Debug, Deserialize)]
pub struct SummaryQuery {
    /// Optional SQL `LIKE` pattern against `alt_type` (e.g. `wyckoff_%`).
    pub alt_type_like: Option<String>,
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    pub profile: Option<String>,
    /// ISO8601 lower bound on `closed_at` for the equity curve window.
    pub since: Option<DateTime<Utc>>,
    /// Max setups folded into the equity curve + breakdown. Safety cap.
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct BacktestSummary {
    pub generated_at: DateTime<Utc>,
    pub total_setups: i64,
    pub armed: i64,
    pub active: i64,
    pub closed: i64,
    pub wins: i64,
    pub losses: i64,
    /// `wins / (wins + losses)` — skips break-even / NULL pnl rows.
    pub hit_rate: Option<f64>,
    pub avg_pnl_pct: Option<f64>,
    /// Sum of `pnl_pct` across closed rows (simple additive; not compounded).
    pub total_pnl_pct: Option<f64>,
    pub equity_curve: Vec<EquityPoint>,
    pub by_alt_type: Vec<AltTypeStat>,
}

#[derive(Debug, Serialize)]
pub struct EquityPoint {
    pub ts: DateTime<Utc>,
    pub cum_pnl_pct: f64,
    pub trade_count: i64,
}

#[derive(Debug, Serialize)]
pub struct AltTypeStat {
    pub alt_type: String,
    pub count: i64,
    pub wins: i64,
    pub losses: i64,
    pub hit_rate: Option<f64>,
    pub avg_pnl_pct: Option<f64>,
    pub total_pnl_pct: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct SetupsListQuery {
    pub limit: Option<i64>,
    pub state: Option<String>,
    pub alt_type_like: Option<String>,
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    pub profile: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BacktestSetupEntry {
    pub id: uuid::Uuid,
    pub created_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub profile: String,
    pub alt_type: Option<String>,
    pub state: String,
    pub direction: String,
    pub entry_price: Option<f32>,
    pub entry_sl: Option<f32>,
    pub target_ref: Option<f32>,
    pub close_price: Option<f32>,
    pub close_reason: Option<String>,
    pub pnl_pct: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct BacktestSetupsResponse {
    pub generated_at: DateTime<Utc>,
    pub entries: Vec<BacktestSetupEntry>,
}

async fn get_summary(
    State(st): State<SharedState>,
    Query(q): Query<SummaryQuery>,
) -> Result<Json<BacktestSummary>, ApiError> {
    let limit = q.limit.unwrap_or(5_000).clamp(1, 50_000);
    let since = q.since;

    // Aggregate counts in one round-trip.
    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        r#"SELECT
             COUNT(*)::BIGINT,
             COUNT(*) FILTER (WHERE state = 'armed')::BIGINT,
             COUNT(*) FILTER (WHERE state = 'active')::BIGINT,
             COUNT(*) FILTER (WHERE state = 'closed')::BIGINT
           FROM qtss_setups
           WHERE mode = 'backtest'
             AND ($1::text IS NULL OR alt_type LIKE $1)
             AND ($2::text IS NULL OR symbol = $2)
             AND ($3::text IS NULL OR timeframe = $3)
             AND ($4::text IS NULL OR profile = $4)"#,
    )
    .bind(q.alt_type_like.as_deref())
    .bind(q.symbol.as_deref())
    .bind(q.timeframe.as_deref())
    .bind(q.profile.as_deref())
    .fetch_one(&st.pool)
    .await?;

    // Closed rows ordered chronologically for equity curve.
    let closed: Vec<(DateTime<Utc>, Option<f32>, Option<String>)> = sqlx::query_as(
        r#"SELECT closed_at, pnl_pct, alt_type
             FROM qtss_setups
            WHERE mode = 'backtest'
              AND state = 'closed'
              AND closed_at IS NOT NULL
              AND pnl_pct IS NOT NULL
              AND ($1::text IS NULL OR alt_type LIKE $1)
              AND ($2::text IS NULL OR symbol = $2)
              AND ($3::text IS NULL OR timeframe = $3)
              AND ($4::text IS NULL OR profile = $4)
              AND ($5::timestamptz IS NULL OR closed_at >= $5)
            ORDER BY closed_at ASC
            LIMIT $6"#,
    )
    .bind(q.alt_type_like.as_deref())
    .bind(q.symbol.as_deref())
    .bind(q.timeframe.as_deref())
    .bind(q.profile.as_deref())
    .bind(since)
    .bind(limit)
    .fetch_all(&st.pool)
    .await?;

    // Equity curve (additive cumulative pnl_pct).
    let mut equity_curve = Vec::with_capacity(closed.len());
    let mut cum = 0.0_f64;
    let mut wins = 0_i64;
    let mut losses = 0_i64;
    let mut pnl_sum = 0.0_f64;
    use std::collections::BTreeMap;
    let mut by_alt: BTreeMap<String, (i64, i64, i64, f64)> = BTreeMap::new();
    for (i, (ts, pnl, alt_type)) in closed.iter().enumerate() {
        let pnl_f = pnl.unwrap_or(0.0) as f64;
        cum += pnl_f;
        pnl_sum += pnl_f;
        if pnl_f > 0.0 {
            wins += 1;
        } else if pnl_f < 0.0 {
            losses += 1;
        }
        equity_curve.push(EquityPoint {
            ts: *ts,
            cum_pnl_pct: cum,
            trade_count: (i as i64) + 1,
        });
        let key = alt_type.clone().unwrap_or_else(|| "unknown".to_string());
        let e = by_alt.entry(key).or_insert((0, 0, 0, 0.0));
        e.0 += 1;
        if pnl_f > 0.0 {
            e.1 += 1;
        } else if pnl_f < 0.0 {
            e.2 += 1;
        }
        e.3 += pnl_f;
    }

    let decided = wins + losses;
    let hit_rate = if decided > 0 {
        Some(wins as f64 / decided as f64)
    } else {
        None
    };
    let n_closed = closed.len() as f64;
    let avg_pnl_pct = if n_closed > 0.0 {
        Some(pnl_sum / n_closed)
    } else {
        None
    };
    let total_pnl_pct = if n_closed > 0.0 { Some(pnl_sum) } else { None };

    let by_alt_type = by_alt
        .into_iter()
        .map(|(alt_type, (count, w, l, sum))| {
            let dec = w + l;
            let hr = if dec > 0 {
                Some(w as f64 / dec as f64)
            } else {
                None
            };
            let avg = if count > 0 {
                Some(sum / count as f64)
            } else {
                None
            };
            AltTypeStat {
                alt_type,
                count,
                wins: w,
                losses: l,
                hit_rate: hr,
                avg_pnl_pct: avg,
                total_pnl_pct: if count > 0 { Some(sum) } else { None },
            }
        })
        .collect();

    Ok(Json(BacktestSummary {
        generated_at: Utc::now(),
        total_setups: counts.0,
        armed: counts.1,
        active: counts.2,
        closed: counts.3,
        wins,
        losses,
        hit_rate,
        avg_pnl_pct,
        total_pnl_pct,
        equity_curve,
        by_alt_type,
    }))
}

async fn get_setups(
    State(st): State<SharedState>,
    Query(q): Query<SetupsListQuery>,
) -> Result<Json<BacktestSetupsResponse>, ApiError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 2_000);

    let rows: Vec<BacktestSetupEntry> = sqlx::query_as(
        r#"SELECT id, created_at, closed_at, exchange, symbol, timeframe, profile,
                  alt_type, state, direction, entry_price, entry_sl, target_ref,
                  close_price, close_reason, pnl_pct
             FROM qtss_setups
            WHERE mode = 'backtest'
              AND ($1::text IS NULL OR state = $1)
              AND ($2::text IS NULL OR alt_type LIKE $2)
              AND ($3::text IS NULL OR symbol = $3)
              AND ($4::text IS NULL OR timeframe = $4)
              AND ($5::text IS NULL OR profile = $5)
            ORDER BY created_at DESC
            LIMIT $6"#,
    )
    .bind(q.state.as_deref())
    .bind(q.alt_type_like.as_deref())
    .bind(q.symbol.as_deref())
    .bind(q.timeframe.as_deref())
    .bind(q.profile.as_deref())
    .bind(limit)
    .fetch_all(&st.pool)
    .await?;

    Ok(Json(BacktestSetupsResponse {
        generated_at: Utc::now(),
        entries: rows,
    }))
}

impl sqlx::FromRow<'_, sqlx::postgres::PgRow> for BacktestSetupEntry {
    fn from_row(row: &sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(Self {
            id: row.try_get("id")?,
            created_at: row.try_get("created_at")?,
            closed_at: row.try_get("closed_at")?,
            exchange: row.try_get("exchange")?,
            symbol: row.try_get("symbol")?,
            timeframe: row.try_get("timeframe")?,
            profile: row.try_get("profile")?,
            alt_type: row.try_get("alt_type")?,
            state: row.try_get("state")?,
            direction: row.try_get("direction")?,
            entry_price: row.try_get("entry_price")?,
            entry_sl: row.try_get("entry_sl")?,
            target_ref: row.try_get("target_ref")?,
            close_price: row.try_get("close_price")?,
            close_reason: row.try_get("close_reason")?,
            pnl_pct: row.try_get("pnl_pct")?,
        })
    }
}
