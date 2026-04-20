//! harmonic_outcome_eval — Faz 12.D
//!
//! Forward-walk evaluator for backtest harmonic detections. For each
//! `qtss_v2_detections` row where:
//!
//!   * `family = 'harmonic'`
//!   * `mode   = 'backtest'`
//!   * no matching `qtss_v2_detection_outcomes` row yet
//!
//! …walks `market_bars` starting from the bar AFTER `detected_at`
//! (which equals the D pivot time) and simulates:
//!
//!   * **Entry**  = close of D pivot bar
//!   * **SL**     = invalidation_price ± `sl_buffer_atr * ATR(D)`
//!   * **TP1**    = Carney-standard retrace of CD leg by `tp1_retrace`
//!                  (default 0.382). Bullish entries target UP from D.
//!   * **TP2**    = same leg with `tp2_retrace` (default 0.618).
//!   * **Time**   = expires after `time_stop_legs × avg_leg_bars(level)`
//!                  bars. `avg_leg_bars` comes from an empirical table
//!                  seeded below; overridable via config later if
//!                  per-symbol tuning proves worthwhile.
//!
//! Outcome mapping:
//!   * TP1/TP2 first → `'win'`
//!   * SL first      → `'loss'`
//!   * Time stop     → `'expired'` (gross P/L at last bar close)
//!
//! `pnl_pct` is **net** of commission (`2 × commission_bps`, round-trip).
//! MEMORY.md commission-gate rule: setups whose gross < commission are
//! already filtered at the setup layer; here we just attribute the
//! realised cost honestly so the level-performance view is actionable.

use std::collections::HashMap;
use std::env;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Empirical median pivot-leg bar counts per level, measured on
/// ~300k BTC/ETH pivots across 5m/15m/1h/4h (Fibo-B ATR multipliers
/// [2,3,5,8]). Used by the time-stop rule. If pivots shift meaningfully
/// in the future this table should move to `config_schema`.
fn avg_leg_bars(level: &str) -> u32 {
    match level {
        "L0" => 7,
        "L1" => 16,
        "L2" => 45,
        "L3" => 115,
        _    => 16,
    }
}

#[derive(Debug)]
struct Row1 {
    id: Uuid,
    exchange: String,
    symbol: String,
    interval: String,
    pivot_level: String,
    subkind: String,
    detected_at: DateTime<Utc>,
    invalidation: Decimal,
    anchors: serde_json::Value,
}

#[derive(Debug)]
struct Bar {
    open_time: DateTime<Utc>,
    high: Decimal,
    low: Decimal,
    close: Decimal,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let dsn = env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL is required"))?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&dsn)
        .await?;

    let cfg = load_config(&pool).await?;
    tracing::info!(?cfg, "harmonic outcome evaluator starting");

    // Batch pull unevaluated backtest rows. 10k per loop keeps memory
    // predictable on large historical sweeps.
    let batch_size: i64 = 10_000;
    let mut total_eval = 0u64;
    loop {
        let batch = pending_batch(&pool, batch_size).await?;
        if batch.is_empty() {
            break;
        }
        tracing::info!(batch = batch.len(), "processing batch");
        // Group by (symbol, interval) so we fetch market_bars once
        // per series, reuse across all detections in that series.
        let mut grouped: HashMap<(String, String, String), Vec<Row1>> = HashMap::new();
        for r in batch {
            grouped
                .entry((r.exchange.clone(), r.symbol.clone(), r.interval.clone()))
                .or_default()
                .push(r);
        }
        for ((exchange, symbol, interval), rows) in grouped {
            // Earliest detection in batch defines how far back to read
            // bars (we only need FORWARD bars after each D time).
            let earliest = rows.iter().map(|r| r.detected_at).min().unwrap();
            let bars = load_bars_after(&pool, &exchange, &symbol, &interval, earliest).await?;
            for row in &rows {
                evaluate_and_record(&pool, row, &bars, &cfg).await?;
                total_eval += 1;
            }
        }
    }
    tracing::info!(total_eval, "outcome eval complete");
    Ok(())
}

#[derive(Debug)]
struct EvalConfig {
    tp1_retrace: f64,
    tp2_retrace: f64,
    sl_buffer_atr: f64,
    time_stop_legs: u32,
    commission_bps: f64,
}

async fn load_config(pool: &PgPool) -> anyhow::Result<EvalConfig> {
    let keys = [
        "backtest.harmonic.tp1_retrace",
        "backtest.harmonic.tp2_retrace",
        "backtest.harmonic.sl_buffer_atr",
        "backtest.harmonic.time_stop_legs",
        "backtest.commission_bps",
    ];
    let mut vals: HashMap<String, f64> = HashMap::new();
    for k in keys {
        // config_schema.default_value is jsonb; treat it as a number
        // and fall back to the textual cast for legacy rows stored as
        // `"0.382"` strings. `config_value` scoping isn't modelled here
        // because backtest jobs always run against global defaults;
        // operators override by editing `config_schema` itself.
        let row = sqlx::query(
            r#"SELECT (default_value::text)::float8 AS v
                 FROM config_schema WHERE key = $1"#,
        )
        .bind(k)
        .fetch_optional(pool)
        .await;
        let v: Option<f64> = row.ok().flatten().and_then(|r| r.try_get("v").ok());
        let f: f64 = v.unwrap_or_else(|| {
            // Hard fallback = in-process Carney defaults. Should only
            // fire if migration 0192 hasn't been applied yet.
            match k {
                "backtest.harmonic.tp1_retrace"    => 0.382,
                "backtest.harmonic.tp2_retrace"    => 0.618,
                "backtest.harmonic.sl_buffer_atr"  => 0.5,
                "backtest.harmonic.time_stop_legs" => 3.0,
                "backtest.commission_bps"          => 4.0,
                _                                  => 0.0,
            }
        });
        vals.insert(k.to_string(), f);
    }
    Ok(EvalConfig {
        tp1_retrace:    vals["backtest.harmonic.tp1_retrace"],
        tp2_retrace:    vals["backtest.harmonic.tp2_retrace"],
        sl_buffer_atr:  vals["backtest.harmonic.sl_buffer_atr"],
        time_stop_legs: vals["backtest.harmonic.time_stop_legs"].round() as u32,
        commission_bps: vals["backtest.commission_bps"],
    })
}

async fn pending_batch(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<Row1>> {
    let rows = sqlx::query(
        r#"SELECT d.id, d.exchange, d.symbol, d.timeframe AS interval,
                  d.pivot_level, d.subkind, d.detected_at,
                  d.invalidation_price AS invalidation, d.anchors
             FROM qtss_v2_detections d
             LEFT JOIN qtss_v2_detection_outcomes o ON o.detection_id = d.id
            WHERE d.family = 'harmonic'
              AND d.mode   = 'backtest'
              AND d.pivot_level IS NOT NULL
              AND o.id IS NULL
            ORDER BY d.detected_at ASC
            LIMIT $1"#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Row1 {
            id:           r.get("id"),
            exchange:     r.get("exchange"),
            symbol:       r.get("symbol"),
            interval:     r.get("interval"),
            pivot_level:  r.get("pivot_level"),
            subkind:      r.get("subkind"),
            detected_at:  r.get("detected_at"),
            invalidation: r.get("invalidation"),
            anchors:      r.get("anchors"),
        })
        .collect())
}

async fn load_bars_after(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    interval: &str,
    after: DateTime<Utc>,
) -> anyhow::Result<Vec<Bar>> {
    let rows = sqlx::query(
        r#"SELECT open_time, high, low, close
             FROM market_bars
            WHERE exchange = $1 AND symbol = $2 AND interval = $3
              AND open_time >= $4
            ORDER BY open_time ASC"#,
    )
    .bind(exchange)
    .bind(symbol)
    .bind(interval)
    .bind(after)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Bar {
            open_time: r.get("open_time"),
            high:      r.get("high"),
            low:       r.get("low"),
            close:     r.get("close"),
        })
        .collect())
}

/// Pull the last N bars up to and including `at` from market_bars.
/// Used for ATR(14) at D to size the SL buffer.
async fn atr14_at(
    pool: &PgPool,
    exchange: &str,
    symbol: &str,
    interval: &str,
    at: DateTime<Utc>,
) -> anyhow::Result<Option<f64>> {
    let rows = sqlx::query(
        r#"SELECT high, low, close
             FROM market_bars
            WHERE exchange = $1 AND symbol = $2 AND interval = $3
              AND open_time <= $4
            ORDER BY open_time DESC
            LIMIT 15"#,
    )
    .bind(exchange)
    .bind(symbol)
    .bind(interval)
    .bind(at)
    .fetch_all(pool)
    .await?;
    if rows.len() < 15 {
        return Ok(None);
    }
    // Wilder TR over 14 periods (row ordering is DESC — reverse once).
    let bars: Vec<(f64, f64, f64)> = rows
        .into_iter()
        .rev()
        .map(|r| {
            (
                r.get::<Decimal, _>("high").to_f64().unwrap_or(0.0),
                r.get::<Decimal, _>("low").to_f64().unwrap_or(0.0),
                r.get::<Decimal, _>("close").to_f64().unwrap_or(0.0),
            )
        })
        .collect();
    let mut trs: Vec<f64> = Vec::with_capacity(14);
    for i in 1..15 {
        let (h, l, _) = bars[i];
        let prev_c = bars[i - 1].2;
        let tr = (h - l).max((h - prev_c).abs()).max((l - prev_c).abs());
        trs.push(tr);
    }
    Ok(Some(trs.iter().sum::<f64>() / 14.0))
}

async fn evaluate_and_record(
    pool: &PgPool,
    row: &Row1,
    bars: &[Bar],
    cfg: &EvalConfig,
) -> anyhow::Result<()> {
    let Some(c_price) = row
        .anchors
        .as_array()
        .and_then(|a| a.get(3))
        .and_then(|c| c.get("price"))
        .and_then(|p| p.as_str())
        .and_then(|s| s.parse::<f64>().ok())
    else {
        return Ok(());
    };
    let Some(d_price) = row
        .anchors
        .as_array()
        .and_then(|a| a.get(4))
        .and_then(|d| d.get("price"))
        .and_then(|p| p.as_str())
        .and_then(|s| s.parse::<f64>().ok())
    else {
        return Ok(());
    };
    let invalidation = row.invalidation.to_f64().unwrap_or(0.0);
    let bearish = row.subkind.ends_with("_bear");

    let atr = atr14_at(pool, &row.exchange, &row.symbol, &row.interval, row.detected_at)
        .await?
        .unwrap_or(0.0);

    // SL = invalidation ± sl_buffer_atr × ATR (away from entry).
    let sl = if bearish {
        invalidation + cfg.sl_buffer_atr * atr
    } else {
        invalidation - cfg.sl_buffer_atr * atr
    };

    // CD leg length; retracement is measured from D back TOWARD C.
    let cd = (d_price - c_price).abs();
    let tp1 = if bearish {
        d_price - cfg.tp1_retrace * cd
    } else {
        d_price + cfg.tp1_retrace * cd
    };
    let tp2 = if bearish {
        d_price - cfg.tp2_retrace * cd
    } else {
        d_price + cfg.tp2_retrace * cd
    };

    // Simulate forward. First bar AFTER D triggers entry semantics —
    // bars[] was fetched with open_time >= detected_at, so skip idx 0
    // (the D bar itself).
    let max_bars = (cfg.time_stop_legs * avg_leg_bars(&row.pivot_level)) as usize;
    let entry = d_price;
    let mut outcome = "expired";
    let mut exit_price = entry;
    let mut exit_time = row.detected_at;

    let start = bars.iter().position(|b| b.open_time > row.detected_at).unwrap_or(bars.len());
    let end = (start + max_bars).min(bars.len());
    for b in &bars[start..end] {
        let hi = b.high.to_f64().unwrap_or(0.0);
        let lo = b.low.to_f64().unwrap_or(0.0);
        let cl = b.close.to_f64().unwrap_or(0.0);
        exit_time = b.open_time;
        // Worst-case intra-bar: if both TP and SL are in the range,
        // we assume SL hit first (conservative — standard backtest
        // convention).
        let hit_sl = if bearish { hi >= sl } else { lo <= sl };
        let hit_tp1 = if bearish { lo <= tp1 } else { hi >= tp1 };
        if hit_sl {
            outcome = "loss";
            exit_price = sl;
            break;
        }
        if hit_tp1 {
            // Check if same bar also reached TP2.
            let hit_tp2 = if bearish { lo <= tp2 } else { hi >= tp2 };
            outcome = "win";
            exit_price = if hit_tp2 { tp2 } else { tp1 };
            break;
        }
        exit_price = cl;
    }

    // P/L in percent; commission = 2× round-trip.
    let gross = if bearish {
        (entry - exit_price) / entry * 100.0
    } else {
        (exit_price - entry) / entry * 100.0
    };
    let commission_pct = 2.0 * cfg.commission_bps / 100.0; // bps → %
    let pnl_pct = gross - commission_pct;
    let duration_secs = (exit_time - row.detected_at).num_seconds().max(0);

    // Map to outcomes-table enum: win/loss/scratch/expired. Keep our
    // own three values ('win', 'loss', 'expired') — 'scratch' is
    // reserved for flat exits a later pass might introduce.
    sqlx::query(
        r#"INSERT INTO qtss_v2_detection_outcomes
               (id, detection_id, outcome, close_reason,
                pnl_pct, entry_price, exit_price, duration_secs, resolved_at)
           VALUES
               (gen_random_uuid(), $1, $2, $3,
                $4, $5, $6, $7, now())
           ON CONFLICT (detection_id) DO NOTHING"#,
    )
    .bind(row.id)
    .bind(outcome)
    .bind(match outcome {
        "win"     => "tp_hit",
        "loss"    => "sl_hit",
        _         => "time_stop",
    })
    .bind(pnl_pct as f32)
    .bind(entry as f32)
    .bind(exit_price as f32)
    .bind(duration_secs)
    .execute(pool)
    .await?;
    Ok(())
}
