//! Persistence layer — write completed `IqBacktestReport` rows to
//! `iq_backtest_runs` so the GUI / ops dashboard can list runs
//! without re-running.
//!
//! The per-trade JSONL log is the source of truth for trade-level
//! analysis; this table is the QUERYABLE INDEX of runs (e.g.
//! "show all BTC 4h dip runs from the last week, sorted by net
//! PnL, with their config snapshots").

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::config::IqBacktestConfig;
use super::report::IqBacktestReport;

/// Insert a completed report into `iq_backtest_runs`. Returns the
/// new run UUID.
pub async fn persist_report(
    pool: &PgPool,
    report: &IqBacktestReport,
    trade_log_path: Option<&str>,
) -> sqlx::Result<Uuid> {
    let id = Uuid::new_v4();
    let cfg_json = serde_json::to_value(&report.config)
        .unwrap_or_else(|_| serde_json::json!({}));
    let polarity_str = match report.config.polarity {
        super::config::IqPolarity::Dip => "dip",
        super::config::IqPolarity::Top => "top",
    };
    let loss_counts = serde_json::to_value(&report.loss_reason_counts)
        .unwrap_or_else(|_| serde_json::json!({}));
    let avg_loss_components = serde_json::to_value(&report.avg_loss_components)
        .unwrap_or_else(|_| serde_json::json!({}));

    sqlx::query(
        r#"INSERT INTO iq_backtest_runs (
              id, run_tag, polarity, exchange, segment, symbol,
              timeframe, start_time, end_time, config,
              bars_processed, total_trades, wins, losses,
              scratches, aborted, open_at_end,
              win_rate, avg_win_pct, avg_loss_pct, profit_factor,
              expectancy_pct, sharpe_ratio,
              gross_pnl, net_pnl, starting_equity, final_equity,
              peak_equity, max_drawdown_pct,
              loss_reason_counts, avg_loss_components,
              trade_log_path
           ) VALUES (
              $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
              $13, $14, $15, $16, $17, $18, $19, $20, $21, $22,
              $23, $24, $25, $26, $27, $28, $29, $30, $31, $32
           )"#,
    )
    .bind(id)
    .bind(&report.config.run_tag)
    .bind(polarity_str)
    .bind(&report.config.universe.exchange)
    .bind(&report.config.universe.segment)
    .bind(&report.config.universe.symbol)
    .bind(&report.config.universe.timeframe)
    .bind(report.config.universe.start_time)
    .bind(report.config.universe.end_time)
    .bind(&cfg_json)
    .bind(report.bars_processed as i64)
    .bind(report.total_trades as i32)
    .bind(report.wins as i32)
    .bind(report.losses as i32)
    .bind(report.scratches as i32)
    .bind(report.aborted as i32)
    .bind(report.open_at_end as i32)
    .bind(report.win_rate)
    .bind(report.avg_win_pct)
    .bind(report.avg_loss_pct)
    .bind(report.profit_factor)
    .bind(report.expectancy_pct)
    .bind(report.sharpe_ratio)
    .bind(report.gross_pnl)
    .bind(report.net_pnl)
    .bind(report.starting_equity)
    .bind(report.final_equity)
    .bind(report.peak_equity)
    .bind(report.max_drawdown_pct)
    .bind(&loss_counts)
    .bind(&avg_loss_components)
    .bind(trade_log_path)
    .execute(pool)
    .await?;
    Ok(id)
}

/// Fetched run row from `iq_backtest_runs` — flat shape suited for
/// the GUI list view. Trade-level detail still in the JSONL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedRun {
    pub id: Uuid,
    pub run_tag: String,
    pub polarity: String,
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub timeframe: String,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: chrono::DateTime<chrono::Utc>,
    pub bars_processed: i64,
    pub total_trades: i32,
    pub wins: i32,
    pub losses: i32,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub net_pnl: Decimal,
    pub max_drawdown_pct: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub trade_log_path: Option<String>,
}

/// List recent runs filtered by optional symbol + tf. `limit`
/// caps the row count (max 200 enforced).
pub async fn list_recent_runs(
    pool: &PgPool,
    symbol: Option<&str>,
    timeframe: Option<&str>,
    limit: u32,
) -> sqlx::Result<Vec<PersistedRun>> {
    let limit = limit.min(200) as i64;
    let rows = sqlx::query(
        r#"SELECT id, run_tag, polarity, exchange, segment, symbol,
                  timeframe, start_time, end_time, bars_processed,
                  total_trades, wins, losses, win_rate, profit_factor,
                  net_pnl, max_drawdown_pct, created_at, trade_log_path
             FROM iq_backtest_runs
            WHERE ($1::text IS NULL OR symbol = $1)
              AND ($2::text IS NULL OR timeframe = $2)
            ORDER BY created_at DESC
            LIMIT $3"#,
    )
    .bind(symbol)
    .bind(timeframe)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(PersistedRun {
            id: r.try_get("id")?,
            run_tag: r.try_get("run_tag")?,
            polarity: r.try_get("polarity")?,
            exchange: r.try_get("exchange")?,
            segment: r.try_get("segment")?,
            symbol: r.try_get("symbol")?,
            timeframe: r.try_get("timeframe")?,
            start_time: r.try_get("start_time")?,
            end_time: r.try_get("end_time")?,
            bars_processed: r.try_get("bars_processed")?,
            total_trades: r.try_get("total_trades")?,
            wins: r.try_get("wins")?,
            losses: r.try_get("losses")?,
            win_rate: r.try_get("win_rate")?,
            profit_factor: r.try_get("profit_factor")?,
            net_pnl: r.try_get("net_pnl")?,
            max_drawdown_pct: r.try_get("max_drawdown_pct")?,
            created_at: r.try_get("created_at")?,
            trade_log_path: r.try_get("trade_log_path").ok(),
        });
    }
    Ok(out)
}

/// Fetch the FULL config + report payload for a single run. Used
/// by the GUI Backtest Studio detail view.
pub async fn get_run_detail(
    pool: &PgPool,
    id: Uuid,
) -> sqlx::Result<Option<(IqBacktestConfig, serde_json::Value)>> {
    let row = sqlx::query(
        r#"SELECT config, loss_reason_counts, avg_loss_components,
                  bars_processed, total_trades, wins, losses,
                  scratches, aborted, open_at_end,
                  win_rate, avg_win_pct, avg_loss_pct, profit_factor,
                  expectancy_pct, sharpe_ratio,
                  gross_pnl, net_pnl, starting_equity, final_equity,
                  peak_equity, max_drawdown_pct,
                  trade_log_path, created_at
             FROM iq_backtest_runs
            WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else { return Ok(None); };
    let cfg_json: serde_json::Value = row.try_get("config")?;
    let cfg: IqBacktestConfig = serde_json::from_value(cfg_json)
        .unwrap_or_default();

    let detail = serde_json::json!({
        "bars_processed":  row.try_get::<i64, _>("bars_processed")?,
        "total_trades":    row.try_get::<i32, _>("total_trades")?,
        "wins":            row.try_get::<i32, _>("wins")?,
        "losses":          row.try_get::<i32, _>("losses")?,
        "scratches":       row.try_get::<i32, _>("scratches")?,
        "aborted":         row.try_get::<i32, _>("aborted")?,
        "open_at_end":     row.try_get::<i32, _>("open_at_end")?,
        "win_rate":        row.try_get::<f64, _>("win_rate")?,
        "avg_win_pct":     row.try_get::<f64, _>("avg_win_pct")?,
        "avg_loss_pct":    row.try_get::<f64, _>("avg_loss_pct")?,
        "profit_factor":   row.try_get::<f64, _>("profit_factor")?,
        "expectancy_pct":  row.try_get::<f64, _>("expectancy_pct")?,
        "sharpe_ratio":    row.try_get::<Option<f64>, _>("sharpe_ratio")?,
        "gross_pnl":       row.try_get::<Decimal, _>("gross_pnl")?,
        "net_pnl":         row.try_get::<Decimal, _>("net_pnl")?,
        "starting_equity": row.try_get::<Decimal, _>("starting_equity")?,
        "final_equity":    row.try_get::<Decimal, _>("final_equity")?,
        "peak_equity":     row.try_get::<Decimal, _>("peak_equity")?,
        "max_drawdown_pct":row.try_get::<f64, _>("max_drawdown_pct")?,
        "loss_reason_counts": row.try_get::<serde_json::Value, _>("loss_reason_counts")?,
        "avg_loss_components": row.try_get::<serde_json::Value, _>("avg_loss_components")?,
        "trade_log_path":  row.try_get::<Option<String>, _>("trade_log_path")?,
        "created_at":      row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")?,
    });
    Ok(Some((cfg, detail)))
}
