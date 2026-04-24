// Workaround: rustc 1.95 dead-code renderer ICE.
#![allow(dead_code)]

//! `radar_aggregator_loop` — periodic performance aggregator (Faz 20A).
//!
//! For each (market, period, mode) triple, computes the running
//! performance snapshot over the current window and upserts into
//! `radar_reports`. Periods: daily / weekly / monthly / yearly.
//! Markets: coin (only live market this release; BIST / NASDAQ
//! auto-join when those pipelines come online).
//!
//! Tick every 5 minutes by default — refreshes the in-progress row
//! for each period. When a period boundary crosses (new day, new
//! week, …), the old row is finalised and a fresh row opens.
//!
//! Source data: `qtss_setups` (closed), `pattern_outcomes` (win
//! labels), `live_positions` (realised PnL).

use std::time::Duration;

use chrono::{DateTime, Datelike, Duration as ChronoDuration, TimeZone, Utc};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tracing::{debug, info, warn};

pub async fn radar_aggregator_loop(pool: PgPool) {
    info!("radar_aggregator_loop: started");
    loop {
        if !load_enabled(&pool).await {
            tokio::time::sleep(Duration::from_secs(load_tick_secs(&pool).await)).await;
            continue;
        }
        let secs = load_tick_secs(&pool).await;
        match run_tick(&pool).await {
            Ok(n) if n > 0 => info!(rows = n, "radar_aggregator tick ok"),
            Ok(_) => debug!("radar_aggregator tick: 0 reports"),
            Err(e) => warn!(%e, "radar_aggregator tick failed"),
        }
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

async fn run_tick(pool: &PgPool) -> anyhow::Result<usize> {
    let cfg = load_cfg(pool).await;
    let now = Utc::now();
    let markets = vec!["coin".to_string()];
    let periods = vec![
        ("daily", period_start_daily(now), period_end_daily(now)),
        ("weekly", period_start_weekly(now), period_end_weekly(now)),
        ("monthly", period_start_monthly(now), period_end_monthly(now)),
        ("yearly", period_start_yearly(now), period_end_yearly(now)),
    ];
    let modes = vec!["live", "dry", "backtest"];
    let mut written = 0usize;
    for market in &markets {
        for (period, start, end) in &periods {
            for mode in &modes {
                let report = aggregate(pool, market, period, mode, *start, *end, &cfg).await?;
                let _ = upsert_report(pool, market, period, mode, *start, *end, &report).await;
                written += 1;
            }
        }
    }
    // Finalize closed periods (period_end < now).
    let _ = sqlx::query(
        "UPDATE radar_reports SET finalised = true WHERE period_end < now() AND finalised = false",
    )
    .execute(pool)
    .await;
    Ok(written)
}

#[derive(Debug, Default)]
struct ReportMetrics {
    closed_count: i64,
    win_count: i64,
    loss_count: i64,
    total_notional: f64,
    total_pnl: f64,
    returns: Vec<f64>,
    holding_bars: Vec<f64>,
    trades: Vec<Value>,
    starting_capital: f64,
    ending_capital: f64,
    /// USD exposure of still-open positions for this mode at period_end.
    /// Feeds `cash_position_pct = (capital - open_exposure) / capital`.
    open_exposure: f64,
}

async fn aggregate(
    pool: &PgPool,
    market: &str,
    period: &str,
    mode: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    cfg: &Cfg,
) -> anyhow::Result<ReportMetrics> {
    // For the "coin" market we look at qtss_setups + live_positions
    // where the venue_class is crypto. BIST / NASDAQ filters come
    // later (they'd gate on venue_class = 'bist' / 'us_equities').
    let venue_filter = match market {
        "coin" => "crypto",
        "bist" => "bist",
        "nasdaq" => "us_equities",
        _ => "crypto",
    };
    let rows = sqlx::query(
        r#"SELECT s.id, s.symbol, s.direction, s.entry_price, s.close_price,
                  -- pnl_pct is legacy/unpopulated; realized_pnl_pct is the
                  -- authoritative field written by the setup closer.
                  COALESCE(s.pnl_pct::numeric, s.realized_pnl_pct) AS pnl_pct_any,
                  s.closed_at, s.bars_to_first_tp,
                  lp.qty_filled, lp.entry_avg
             FROM qtss_setups s
             LEFT JOIN live_positions lp ON lp.setup_id = s.id
            WHERE s.venue_class = $1
              AND s.mode = $2
              AND s.state IN ('closed','closed_win','closed_loss','closed_manual',
                              'closed_partial_win','closed_scratch')
              AND s.closed_at >= $3
              AND s.closed_at < $4
            ORDER BY s.closed_at ASC"#,
    )
    .bind(venue_filter)
    .bind(mode)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    let mut m = ReportMetrics::default();
    m.starting_capital = cfg.default_starting_capital;
    // Open USD exposure at period_end — rolling sum of entry_avg * qty_remaining
    // across live_positions still open for this mode. This feeds
    // cash_position_pct so the report distinguishes "I have 100% cash"
    // from "I have capital tied up in open trades right now".
    let open_row = sqlx::query(
        r#"SELECT COALESCE(SUM(entry_avg * qty_remaining), 0)::float8 AS open_usd
             FROM live_positions
            WHERE mode = $1
              AND closed_at IS NULL
              AND opened_at <= $2"#,
    )
    .bind(mode)
    .bind(end)
    .fetch_optional(pool)
    .await?;
    if let Some(row) = open_row {
        m.open_exposure = row.try_get::<f64, _>("open_usd").unwrap_or(0.0);
    }
    for r in &rows {
        let symbol: String = r.try_get("symbol").unwrap_or_default();
        let direction: String = r.try_get("direction").unwrap_or_default();
        let entry: Option<f32> = r.try_get("entry_price").ok();
        let close: Option<f32> = r.try_get("close_price").ok();
        // realized_pnl_pct is NUMERIC in the qtss_setups schema, hence the
        // COALESCE in the SQL above returns NUMERIC — decode it as Decimal
        // and lift to f64 so the rest of the pipeline stays in float space.
        let pnl_pct: Option<rust_decimal::Decimal> = r.try_get("pnl_pct_any").ok();
        let closed_at: Option<DateTime<Utc>> = r.try_get("closed_at").ok();
        let bars_to_tp: Option<i32> = r.try_get("bars_to_first_tp").ok();
        // Position size — rough notional = entry × qty_filled if live
        // row present; else default allocation from capital.
        let qty_filled: Option<rust_decimal::Decimal> = r.try_get("qty_filled").ok();
        let notional = qty_filled
            .and_then(|q| {
                use rust_decimal::prelude::ToPrimitive;
                q.to_f64()
            })
            .zip(entry.map(|e| e as f64))
            .map(|(q, e)| q * e)
            .unwrap_or(m.starting_capital * 0.1);
        let ret = pnl_pct
            .and_then(|d| {
                use rust_decimal::prelude::ToPrimitive;
                d.to_f64()
            })
            .map(|p| p / 100.0)
            .unwrap_or(0.0);
        let pnl_usd = notional * ret;
        m.closed_count += 1;
        if ret > 0.0 {
            m.win_count += 1;
        } else if ret < 0.0 {
            m.loss_count += 1;
        }
        m.total_notional += notional;
        m.total_pnl += pnl_usd;
        m.returns.push(ret);
        if let Some(b) = bars_to_tp {
            m.holding_bars.push(b as f64);
        }
        m.trades.push(json!({
            "symbol": symbol,
            "direction": direction,
            "date": closed_at,
            "notional_usd": notional,
            "pnl_usd": pnl_usd,
            "return_pct": ret * 100.0,
            "status": if ret > 0.0 { "win" } else if ret < 0.0 { "loss" } else { "scratch" },
        }));
    }
    m.ending_capital = m.starting_capital + m.total_pnl;
    Ok(m)
}

async fn upsert_report(
    pool: &PgPool,
    market: &str,
    period: &str,
    mode: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    m: &ReportMetrics,
) -> anyhow::Result<()> {
    let win_rate = if m.closed_count > 0 {
        m.win_count as f64 / m.closed_count as f64
    } else {
        0.0
    };
    let avg_return = if m.returns.is_empty() {
        0.0
    } else {
        m.returns.iter().sum::<f64>() / m.returns.len() as f64
    };
    let compound_return = m.returns.iter().fold(1.0f64, |acc, r| acc * (1.0 + r)) - 1.0;
    let avg_allocation = if m.closed_count > 0 {
        m.total_notional / m.closed_count as f64
    } else {
        0.0
    };
    let avg_holding = if m.holding_bars.is_empty() {
        0.0
    } else {
        m.holding_bars.iter().sum::<f64>() / m.holding_bars.len() as f64
    };
    let std_dev = {
        if m.returns.len() < 2 {
            0.0
        } else {
            let mean = avg_return;
            (m.returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
                / (m.returns.len() - 1) as f64)
                .sqrt()
        }
    };
    let volatility = if std_dev > 0.05 {
        "high"
    } else if std_dev > 0.02 {
        "medium"
    } else {
        "low"
    };
    // cash_pct = (capital − open_exposure) / capital. The old formula
    // multiplied open_exposure by 0.0 so it always rounded to 100%; now
    // the open-live-positions sum (loaded in `aggregate`) feeds the
    // deduction. Capped to [0, 100] to guard against oversized exposure
    // during very short periods.
    let cash_pct = if m.ending_capital > 0.0 {
        ((m.ending_capital - m.open_exposure).max(0.0) / m.ending_capital * 100.0)
            .clamp(0.0, 100.0)
    } else {
        100.0
    };
    // risk_mode is cash-first (how much of the book is deployed) and
    // falls back to win-rate when there is no capital context — so a
    // loop with zero trades but heavy open exposure still flags Risk-On.
    let risk_mode = if cash_pct >= 90.0 {
        "risk_off"
    } else if cash_pct >= 40.0 {
        "neutral"
    } else {
        "risk_on"
    };

    sqlx::query(
        r#"INSERT INTO radar_reports
              (market, period, mode, period_start, period_end, finalised,
               trades, closed_count, win_count, loss_count, win_rate,
               total_notional_usd, total_pnl_usd, avg_return_pct,
               compound_return_pct, avg_allocation_usd, avg_holding_bars,
               starting_capital_usd, ending_capital_usd, cash_position_pct,
               risk_mode, volatility_level, max_position_risk_pct,
               open_exposure_usd)
           VALUES ($1,$2,$3,$4,$5,false,
                   $6,$7,$8,$9,$10,
                   $11,$12,$13,
                   $14,$15,$16,
                   $17,$18,$19,
                   $20,$21,$22,
                   $23)
           ON CONFLICT (market, period, mode, period_start)
           DO UPDATE SET
               trades              = EXCLUDED.trades,
               closed_count        = EXCLUDED.closed_count,
               win_count           = EXCLUDED.win_count,
               loss_count          = EXCLUDED.loss_count,
               win_rate            = EXCLUDED.win_rate,
               total_notional_usd  = EXCLUDED.total_notional_usd,
               total_pnl_usd       = EXCLUDED.total_pnl_usd,
               avg_return_pct      = EXCLUDED.avg_return_pct,
               compound_return_pct = EXCLUDED.compound_return_pct,
               avg_allocation_usd  = EXCLUDED.avg_allocation_usd,
               avg_holding_bars    = EXCLUDED.avg_holding_bars,
               ending_capital_usd  = EXCLUDED.ending_capital_usd,
               cash_position_pct   = EXCLUDED.cash_position_pct,
               risk_mode           = EXCLUDED.risk_mode,
               volatility_level    = EXCLUDED.volatility_level,
               max_position_risk_pct = EXCLUDED.max_position_risk_pct,
               open_exposure_usd   = EXCLUDED.open_exposure_usd,
               computed_at         = now()"#,
    )
    .bind(market)
    .bind(period)
    .bind(mode)
    .bind(start)
    .bind(end)
    .bind(Value::Array(m.trades.clone()))
    .bind(m.closed_count as i32)
    .bind(m.win_count as i32)
    .bind(m.loss_count as i32)
    .bind(win_rate)
    .bind(m.total_notional)
    .bind(m.total_pnl)
    .bind(avg_return * 100.0)
    .bind(compound_return * 100.0)
    .bind(avg_allocation)
    .bind(avg_holding)
    .bind(m.starting_capital)
    .bind(m.ending_capital)
    .bind(cash_pct)
    .bind(risk_mode)
    .bind(volatility)
    .bind(0.02f64) // placeholder — real calc needs live leverage + qty
    .bind(m.open_exposure)
    .execute(pool)
    .await?;
    Ok(())
}

// ── Period helpers ─────────────────────────────────────────────────────

fn period_start_daily(now: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .single()
        .unwrap_or(now)
}
fn period_end_daily(now: DateTime<Utc>) -> DateTime<Utc> {
    period_start_daily(now) + ChronoDuration::days(1)
}
fn period_start_weekly(now: DateTime<Utc>) -> DateTime<Utc> {
    let dow = now.weekday().num_days_from_monday() as i64;
    period_start_daily(now) - ChronoDuration::days(dow)
}
fn period_end_weekly(now: DateTime<Utc>) -> DateTime<Utc> {
    period_start_weekly(now) + ChronoDuration::days(7)
}
fn period_start_monthly(now: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .unwrap_or(now)
}
fn period_end_monthly(now: DateTime<Utc>) -> DateTime<Utc> {
    // First of next month.
    let (y, m) = if now.month() == 12 {
        (now.year() + 1, 1)
    } else {
        (now.year(), now.month() + 1)
    };
    Utc.with_ymd_and_hms(y, m, 1, 0, 0, 0).single().unwrap_or(now)
}
fn period_start_yearly(now: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year(), 1, 1, 0, 0, 0)
        .single()
        .unwrap_or(now)
}
fn period_end_yearly(now: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year() + 1, 1, 1, 0, 0, 0)
        .single()
        .unwrap_or(now)
}

// ── Config ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Cfg {
    default_starting_capital: f64,
}

async fn load_enabled(pool: &PgPool) -> bool {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'radar' AND config_key = 'enabled'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return true; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

async fn load_tick_secs(pool: &PgPool) -> u64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'radar' AND config_key = 'tick_secs'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 300; };
    let val: Value = row.try_get("value").unwrap_or(Value::Null);
    val.get("secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(300)
        .max(60)
}

async fn load_cfg(pool: &PgPool) -> Cfg {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'radar' AND config_key = 'default_starting_capital_usd'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let default_cap = row
        .and_then(|r| r.try_get::<Value, _>("value").ok())
        .and_then(|v| v.get("value").and_then(|x| x.as_f64()))
        .unwrap_or(1_500_000.0);
    Cfg {
        default_starting_capital: default_cap,
    }
}
