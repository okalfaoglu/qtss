//! wyckoff_backtest_sweep — Faz 12.C (wyckoff extension).
//!
//! Unlike harmonic/classical/elliott which are stateless geometric
//! matchers, Wyckoff events are *tail-only* by construction — each
//! `EventSpec::eval` inspects the trailing `pivot_window` pivots and
//! the matching bar slice. A single call on the full pivot history
//! therefore only yields the events visible right now.
//!
//! To get the full historical inventory we walk pivot-by-pivot: at
//! every pivot completion we take the last `pivot_window` pivots and
//! the bars that cover them, run every `EVENTS` entry, and emit any
//! new firings. A signature-based dedup (`event_name + variant +
//! first/last anchor time`) suppresses the repeat emissions that
//! happen when the same event stays valid across several pivot steps.
//!
//! Output contract matches the sibling sweep binaries:
//!   * `family = 'wyckoff'`
//!   * `mode   = 'backtest'`
//!   * `pivot_level = 'L0'..'L3'`
//!   * `raw_meta.run_id` = UUID of this invocation.
//!
//! Env overrides (optional):
//!   * `WYCKOFF_SWEEP_SYMBOLS`    — CSV filter
//!   * `WYCKOFF_SWEEP_INTERVALS`  — CSV filter
//!   * `WYCKOFF_SWEEP_MIN_SCORE`  — score floor (default: config)
//!   * `WYCKOFF_SWEEP_WINDOW`     — trailing pivot window (default: config)

use std::collections::HashSet;
use std::env;

use chrono::{DateTime, Utc};
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::pivot::{Pivot, PivotKind, PivotLevel};
use qtss_domain::v2::timeframe::Timeframe;
use qtss_wyckoff::{EventContext, EventEval, EventMatch, WyckoffConfig, EVENTS};
use rust_decimal::Decimal;
use serde_json::{json, Value as Json};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

const LEVELS: [PivotLevel; 4] = [PivotLevel::L0, PivotLevel::L1, PivotLevel::L2, PivotLevel::L3];

fn pivot_level_to_i16(level: PivotLevel) -> i16 {
    match level {
        PivotLevel::L0 => 0,
        PivotLevel::L1 => 1,
        PivotLevel::L2 => 2,
        PivotLevel::L3 => 3,
        PivotLevel::L4 => 4,
    }
}

#[derive(Debug)]
struct SeriesKey {
    exchange: String,
    segment: String,
    symbol: String,
    interval: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let dsn = env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL is required"))?;
    let pool = PgPoolOptions::new().max_connections(4).connect(&dsn).await?;

    let run_id = Uuid::new_v4();
    let cfg = WyckoffConfig::defaults();
    let min_score_override: Option<f32> = env::var("WYCKOFF_SWEEP_MIN_SCORE")
        .ok()
        .and_then(|v| v.parse().ok());
    let window_override: Option<usize> = env::var("WYCKOFF_SWEEP_WINDOW")
        .ok()
        .and_then(|v| v.parse().ok());
    let min_score = min_score_override.unwrap_or(cfg.min_structural_score);
    let window = window_override.unwrap_or_else(|| cfg.pivot_window.max(cfg.min_range_pivots));

    let symbol_filter: Option<Vec<String>> = env::var("WYCKOFF_SWEEP_SYMBOLS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let interval_filter: Option<Vec<String>> = env::var("WYCKOFF_SWEEP_INTERVALS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());

    tracing::info!(
        run_id = %run_id, min_score, window,
        symbols = ?symbol_filter, intervals = ?interval_filter,
        "wyckoff backtest sweep starting"
    );

    let series = list_series(&pool, symbol_filter.as_deref(), interval_filter.as_deref()).await?;
    tracing::info!(count = series.len(), "series enumerated");

    let mut total: u64 = 0;
    for s in &series {
        let bars = load_bars(&pool, s).await?;
        if bars.is_empty() {
            continue;
        }
        for level in LEVELS {
            let pivots = load_pivots(&pool, s, level).await?;
            if pivots.len() < cfg.min_range_pivots {
                continue;
            }
            let n = sweep_series(&pool, s, level, &pivots, &bars, &cfg, min_score, window, run_id)
                .await?;
            if n > 0 {
                tracing::info!(
                    symbol = %s.symbol, interval = %s.interval, level = level.as_str(),
                    pivots = pivots.len(), bars = bars.len(), inserted = n,
                    "sweep batch done"
                );
                total += n as u64;
            }
        }
    }
    tracing::info!(total_inserted = total, run_id = %run_id, "sweep complete");
    Ok(())
}

async fn list_series(
    pool: &PgPool,
    symbols: Option<&[String]>,
    intervals: Option<&[String]>,
) -> anyhow::Result<Vec<SeriesKey>> {
    let rows = sqlx::query(
        r#"SELECT DISTINCT mb.exchange, mb.segment, mb.symbol, mb.interval
             FROM market_bars mb
             JOIN engine_symbols es
               ON es.exchange = mb.exchange
              AND es.symbol   = mb.symbol
              AND es."interval" = mb.interval
             JOIN pivots p ON p.engine_symbol_id = es.id
            WHERE ($1::text[] IS NULL OR mb.symbol   = ANY($1))
              AND ($2::text[] IS NULL OR mb.interval = ANY($2))
            ORDER BY mb.exchange, mb.symbol, mb.interval"#,
    )
    .bind(symbols)
    .bind(intervals)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| SeriesKey {
            exchange: r.get("exchange"),
            segment: r.get("segment"),
            symbol: r.get("symbol"),
            interval: r.get("interval"),
        })
        .collect())
}

async fn load_pivots(
    pool: &PgPool,
    s: &SeriesKey,
    level: PivotLevel,
) -> anyhow::Result<Vec<Pivot>> {
    let level_i: i16 = pivot_level_to_i16(level);
    let rows = sqlx::query(
        r#"SELECT p.bar_index, p.open_time, p.price, p.direction,
                  p.prominence, p.volume
             FROM pivots p
             JOIN engine_symbols es ON es.id = p.engine_symbol_id
            WHERE es.exchange   = $1
              AND es.symbol     = $2
              AND es."interval" = $3
              AND p.level       = $4
            ORDER BY p.bar_index ASC"#,
    )
    .bind(&s.exchange)
    .bind(&s.symbol)
    .bind(&s.interval)
    .bind(level_i)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let direction: i16 = r.get("direction");
            let bar_index: i64 = r.get("bar_index");
            let open_time: DateTime<Utc> = r.get("open_time");
            Pivot {
                bar_index: bar_index as u64,
                time: open_time,
                price: r.get("price"),
                kind: if direction >= 1 { PivotKind::High } else { PivotKind::Low },
                level,
                prominence: r.get("prominence"),
                volume_at_pivot: r.get("volume"),
                swing_type: None,
            }
        })
        .collect())
}

async fn load_bars(pool: &PgPool, s: &SeriesKey) -> anyhow::Result<Vec<Bar>> {
    // Minimal Instrument — Wyckoff events only look at OHLCV + time, not
    // venue fields. Using a stub keeps the sweep asset-class agnostic.
    let instrument = stub_instrument(&s.exchange, &s.symbol);
    let tf = parse_timeframe(&s.interval).unwrap_or(Timeframe::H1);
    let rows = sqlx::query(
        r#"SELECT open_time, open, high, low, close, volume
             FROM market_bars
            WHERE exchange=$1 AND segment=$2 AND symbol=$3 AND interval=$4
            ORDER BY open_time ASC"#,
    )
    .bind(&s.exchange)
    .bind(&s.segment)
    .bind(&s.symbol)
    .bind(&s.interval)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Bar {
            instrument: instrument.clone(),
            timeframe: tf,
            open_time: r.get("open_time"),
            open: r.get("open"),
            high: r.get("high"),
            low: r.get("low"),
            close: r.get("close"),
            volume: r.get("volume"),
            closed: true,
        })
        .collect())
}

fn stub_instrument(exchange: &str, symbol: &str) -> Instrument {
    let venue = if exchange.eq_ignore_ascii_case("binance") {
        Venue::Binance
    } else {
        Venue::Custom(exchange.to_string())
    };
    Instrument {
        venue,
        asset_class: AssetClass::CryptoSpot,
        symbol: symbol.to_string(),
        quote_ccy: "USDT".to_string(),
        tick_size: Decimal::new(1, 8),
        lot_size: Decimal::new(1, 8),
        session: SessionCalendar::binance_24x7(),
    }
}

fn parse_timeframe(interval: &str) -> Option<Timeframe> {
    match interval.trim().to_lowercase().as_str() {
        "1m" => Some(Timeframe::M1),
        "5m" => Some(Timeframe::M5),
        "15m" => Some(Timeframe::M15),
        "30m" => Some(Timeframe::M30),
        "1h" => Some(Timeframe::H1),
        "4h" => Some(Timeframe::H4),
        "1d" => Some(Timeframe::D1),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
async fn sweep_series(
    pool: &PgPool,
    s: &SeriesKey,
    level: PivotLevel,
    pivots: &[Pivot],
    bars: &[Bar],
    cfg: &WyckoffConfig,
    min_score: f32,
    window: usize,
    run_id: Uuid,
) -> anyhow::Result<usize> {
    sqlx::query(
        r#"DELETE FROM qtss_v2_detections
            WHERE mode = 'backtest'
              AND family = 'wyckoff'
              AND exchange = $1 AND symbol = $2
              AND timeframe = $3 AND pivot_level = $4"#,
    )
    .bind(&s.exchange)
    .bind(&s.symbol)
    .bind(&s.interval)
    .bind(level.as_str())
    .execute(pool)
    .await?;

    // Dedup signature: (event_name, variant, first_anchor_time, last_anchor_time).
    // Same event can fire on many pivot-step windows while its anchors
    // stay stable — we only want to persist one copy.
    let mut seen: HashSet<(String, String, i64, i64)> = HashSet::new();
    let mut inserted = 0usize;

    // Walk pivot-by-pivot: each step represents "simulated present" at
    // that pivot's close. `end` is exclusive.
    let start_step = cfg.min_range_pivots.max(window / 2);
    for end in start_step..=pivots.len() {
        let begin = end.saturating_sub(window);
        let window_pivots = &pivots[begin..end];
        if window_pivots.len() < cfg.min_range_pivots {
            continue;
        }
        let cutoff_time = window_pivots.last().map(|p| p.time).unwrap_or_else(Utc::now);
        // Bar slice up to the last pivot's time (walk-forward correct).
        let bar_end = bars.partition_point(|b| b.open_time <= cutoff_time);
        let bar_slice = &bars[..bar_end];
        let ctx = EventContext::new(window_pivots, bar_slice, cfg);

        for spec in EVENTS {
            let m_opt: Option<EventMatch> = match spec.eval {
                EventEval::Pivots(f) => f(window_pivots, cfg),
                EventEval::WithBars(f) => f(&ctx),
            };
            let Some(m) = m_opt else { continue };
            if (m.score as f32) < min_score {
                continue;
            }
            // Map anchor_labels (trailing-pivot labels, oldest..newest)
            // back to the actual pivots they reference.
            let take = m.anchor_labels.len().min(window_pivots.len());
            if take == 0 {
                continue;
            }
            let tail = &window_pivots[window_pivots.len() - take..];
            let sig = (
                spec.name.to_string(),
                m.variant.to_string(),
                tail.first().unwrap().time.timestamp(),
                tail.last().unwrap().time.timestamp(),
            );
            if !seen.insert(sig) {
                continue;
            }
            let subkind = format!("{}_{}", spec.name, m.variant);
            insert_detection(pool, s, level, tail, &m.anchor_labels, &m, &subkind, run_id).await?;
            inserted += 1;
        }
    }
    Ok(inserted)
}

#[allow(clippy::too_many_arguments)]
async fn insert_detection(
    pool: &PgPool,
    s: &SeriesKey,
    level: PivotLevel,
    tail: &[Pivot],
    labels: &[&'static str],
    m: &EventMatch,
    subkind: &str,
    run_id: Uuid,
) -> anyhow::Result<()> {
    let anchors: Json = json!(tail
        .iter()
        .zip(labels.iter())
        .map(|(p, label)| json!({
            "bar_index": p.bar_index,
            "price": p.price.to_string(),
            "level": level.as_str(),
            "label": label,
            "time": p.time.to_rfc3339(),
        }))
        .collect::<Vec<_>>());

    let raw_meta: Json = json!({
        "run_id": run_id.to_string(),
        "sweep": "wyckoff_backtest_sweep",
        "variant": m.variant,
    });
    let regime: Json = json!({ "backtest": true });

    let detected_at = tail.last().map(|p| p.time).unwrap_or_else(Utc::now);

    sqlx::query(
        r#"INSERT INTO qtss_v2_detections (
               id, detected_at, exchange, symbol, timeframe,
               family, subkind, state, structural_score,
               invalidation_price, anchors, regime, raw_meta, mode,
               pivot_level
           ) VALUES (
               $1, $2, $3, $4, $5,
               'wyckoff', $6, 'confirmed', $7,
               $8, $9, $10, $11, 'backtest',
               $12
           )"#,
    )
    .bind(Uuid::new_v4())
    .bind(detected_at)
    .bind(&s.exchange)
    .bind(&s.symbol)
    .bind(&s.interval)
    .bind(subkind)
    .bind(m.score as f32)
    .bind(m.invalidation)
    .bind(&anchors)
    .bind(&regime)
    .bind(&raw_meta)
    .bind(level.as_str())
    .execute(pool)
    .await?;
    Ok(())
}
