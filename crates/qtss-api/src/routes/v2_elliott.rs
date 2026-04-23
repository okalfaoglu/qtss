//! `GET /v2/elliott/{venue}/{symbol}/{tf}` — canonical Elliott Wave
//! endpoint. Runs the LuxAlgo motive/ABC/fib/break-box state machine
//! on top of the system-wide trailing-window zigzag and returns a
//! plain-data snapshot the chart can render directly.
//!
//! This endpoint replaces what used to run client-side in
//! `web/src/lib/luxalgo-pine-port.ts`. The frontend now only draws;
//! every pivot and formation comes from the same Rust code the worker
//! uses to write detections to the database — one source of truth.
//!
//! Candle loading mirrors `/v2/zigzag` bar-for-bar (same bar_index
//! alignment) so the chart can layer this endpoint's output on top of
//! the zigzag response without any re-indexing.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use qtss_elliott::luxalgo_pine_port::{
    run as run_pine_port, Bar as PortBar, HiSource, LevelConfig, LoSource, PinePortConfig,
    PinePortOutput,
};
use qtss_storage::{market_bars, market_bars_open};

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct ElliottQuery {
    pub limit: Option<i64>,
    pub segment: Option<String>,
    /// Comma-separated zigzag lengths (e.g. `3,5,8`). The frontend
    /// passes the same list it uses for the zigzag overlay so bar
    /// indices line up slot-for-slot.
    pub lengths: Option<String>,
    /// Comma-separated colors, parallel to `lengths` (ignored if
    /// shorter). Purely cosmetic — drives the `color` field on each
    /// level in the response.
    pub colors: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ElliottCandle {
    pub time: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub bar_index: i64,
}

#[derive(Debug, Serialize)]
pub struct ElliottResponse {
    pub venue: String,
    pub symbol: String,
    pub timeframe: String,
    pub candles: Vec<ElliottCandle>,
    #[serde(flatten)]
    pub pine: PinePortOutput,
}

pub fn v2_elliott_router() -> Router<SharedState> {
    Router::new().route("/v2/elliott/{venue}/{symbol}/{tf}", get(get_elliott))
}

/// Mirror of the zigzag route's loader — same config key so the two
/// endpoints agree on which swings survive the filter.
async fn load_min_prominence_pct(pool: &sqlx::PgPool) -> f64 {
    let row = sqlx::query(
        "SELECT value FROM system_config WHERE module = 'zigzag' AND config_key = 'min_prominence_pct'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return 0.0; };
    let val: serde_json::Value = row.try_get("value").unwrap_or(serde_json::Value::Null);
    val.get("pct")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        .max(0.0)
}

fn parse_lengths(csv: Option<&str>) -> Vec<usize> {
    csv.map(|s| {
        s.split(',')
            .filter_map(|part| part.trim().parse::<usize>().ok())
            .filter(|&n| n >= 1)
            .collect::<Vec<_>>()
    })
    .filter(|v: &Vec<usize>| !v.is_empty())
    .unwrap_or_else(|| vec![3, 5, 8])
}

fn parse_colors(csv: Option<&str>) -> Vec<String> {
    csv.map(|s| {
        s.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
    })
    .unwrap_or_default()
}

async fn get_elliott(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<ElliottQuery>,
) -> Result<Json<ElliottResponse>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(1000).clamp(1, 5_000);

    let rows =
        market_bars::list_recent_bars(&st.pool, &venue, &segment, &symbol, &tf, limit).await?;

    // DB returns newest-first — flip to chronological.
    let mut candles: Vec<ElliottCandle> = rows
        .into_iter()
        .rev()
        .enumerate()
        .map(|(i, r)| ElliottCandle {
            time: r.open_time,
            open: r.open,
            high: r.high,
            low: r.low,
            close: r.close,
            volume: r.volume,
            bar_index: i as i64,
        })
        .collect();

    // Append the live bar from `market_bars_open` so the state machine
    // can emit pivots on the still-forming candle (matches the zigzag
    // endpoint — otherwise bar_index would drift between the two).
    if let Ok(Some(open_bar)) =
        market_bars_open::get_open_bar(&st.pool, &venue, &segment, &symbol, &tf).await
    {
        let is_newer = candles
            .last()
            .map(|c| open_bar.open_time > c.time)
            .unwrap_or(true);
        if is_newer {
            let next_idx = candles.len() as i64;
            candles.push(ElliottCandle {
                time: open_bar.open_time,
                open: open_bar.open,
                high: open_bar.high,
                low: open_bar.low,
                close: open_bar.close,
                volume: open_bar.volume,
                bar_index: next_idx,
            });
        }
    }

    let bars: Vec<PortBar> = candles
        .iter()
        .map(|c| PortBar {
            open: c.open.to_f64().unwrap_or(0.0),
            high: c.high.to_f64().unwrap_or(0.0),
            low: c.low.to_f64().unwrap_or(0.0),
            close: c.close.to_f64().unwrap_or(0.0),
        })
        .collect();

    let lengths = parse_lengths(q.lengths.as_deref());
    let colors = parse_colors(q.colors.as_deref());
    let defaults = ["#ef4444", "#3b82f6", "#e5e7eb", "#f59e0b", "#a78bfa"];
    let min_prominence_pct = load_min_prominence_pct(&st.pool).await;
    let cfg = PinePortConfig {
        hi_source: HiSource::High,
        lo_source: LoSource::Low,
        levels: lengths
            .into_iter()
            .enumerate()
            .map(|(i, length)| LevelConfig {
                length,
                color: colors
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| defaults[i % defaults.len()].to_string()),
            })
            .collect(),
        min_prominence_pct,
            ..PinePortConfig::default()
    };

    let pine = run_pine_port(&bars, &cfg);

    Ok(Json(ElliottResponse {
        venue,
        symbol,
        timeframe: tf,
        candles,
        pine,
    }))
}
