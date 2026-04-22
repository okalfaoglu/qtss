//! `GET /v2/harmonic-db/{venue}/{symbol}/{tf}` — harmonic patterns
//! (Gartley, Bat, Butterfly, Crab, Cypher, ...) read from the
//! persisted `detections` table. Returns the same shape the chart
//! already uses for Elliott: candles + per-pattern anchor arrays
//! with `time` fields so the frontend can remap `bar_index` into
//! its current window on the fly.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use qtss_storage::market_bars;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct HarmonicDbQuery {
    pub limit: Option<i64>,
    pub segment: Option<String>,
    /// Restrict to a single slot (0..=4). Omit for all.
    pub slot: Option<i16>,
    /// Restrict to a single harmonic subkind like "cypher_bull".
    pub subkind: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct HarmonicCandle {
    pub time: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub bar_index: i64,
}

#[derive(Debug, Serialize)]
pub struct HarmonicAnchor {
    pub bar_index: i64,
    pub time: DateTime<Utc>,
    pub price: f64,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct HarmonicPattern {
    pub slot: i16,
    /// `"gartley_bull" | "bat_bear" | "cypher_bull" | ...`
    pub subkind: String,
    pub direction: i16,
    pub start_bar: i64,
    pub end_bar: i64,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub invalidated: bool,
    /// X, A, B, C, D in order.
    pub anchors: Vec<HarmonicAnchor>,
    /// Ratios + score passthrough from raw_meta for chart tooltips.
    pub score: Option<f64>,
    pub ratios: Option<serde_json::Value>,
    pub extension: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct HarmonicResponse {
    pub venue: String,
    pub symbol: String,
    pub timeframe: String,
    pub candles: Vec<HarmonicCandle>,
    pub patterns: Vec<HarmonicPattern>,
}

pub fn v2_harmonic_db_router() -> Router<SharedState> {
    Router::new().route(
        "/v2/harmonic-db/{venue}/{symbol}/{tf}",
        get(get_harmonic_db),
    )
}

async fn get_harmonic_db(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<HarmonicDbQuery>,
) -> Result<Json<HarmonicResponse>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(1000).clamp(1, 5_000);

    // Candles — same loader as /v2/elliott-db so bar_index alignment is
    // guaranteed between the two endpoints.
    let rows =
        market_bars::list_recent_bars(&st.pool, &venue, &segment, &symbol, &tf, limit).await?;
    let candles: Vec<HarmonicCandle> = rows
        .into_iter()
        .rev()
        .enumerate()
        .map(|(i, r)| HarmonicCandle {
            time: r.open_time,
            open: r.open,
            high: r.high,
            low: r.low,
            close: r.close,
            volume: r.volume,
            bar_index: i as i64,
        })
        .collect();

    let raw_rows = sqlx::query(
        r#"SELECT slot, subkind, direction, start_bar, end_bar,
                  start_time, end_time, anchors, invalidated, raw_meta
             FROM detections
            WHERE exchange = $1 AND segment = $2 AND symbol = $3
              AND timeframe = $4 AND pattern_family = 'harmonic'
              AND mode = 'live'
              AND ($5::smallint IS NULL OR slot = $5)
              AND ($6::text IS NULL OR subkind = $6)
            ORDER BY start_time"#,
    )
    .bind(&venue)
    .bind(&segment)
    .bind(&symbol)
    .bind(&tf)
    .bind(q.slot)
    .bind(q.subkind.as_deref())
    .fetch_all(&st.pool)
    .await?;

    let mut patterns: Vec<HarmonicPattern> = Vec::with_capacity(raw_rows.len());
    for row in raw_rows {
        let anchors_json: serde_json::Value = row.try_get("anchors").unwrap_or(serde_json::Value::Null);
        let raw_meta: serde_json::Value = row.try_get("raw_meta").unwrap_or(serde_json::Value::Null);
        let anchors = match remap_anchors(&anchors_json, &candles) {
            Some(a) if a.len() == 5 => a,
            _ => continue, // Out-of-window or malformed — skip rather than render wrong.
        };
        // Refresh start_bar / end_bar to the remapped window indices so
        // clients that use them (stop-loss zone rendering, etc.) stay
        // consistent.
        let start_bar = anchors.first().map(|a| a.bar_index).unwrap_or_else(|| row.get("start_bar"));
        let end_bar = anchors.last().map(|a| a.bar_index).unwrap_or_else(|| row.get("end_bar"));
        patterns.push(HarmonicPattern {
            slot: row.get("slot"),
            subkind: row.get("subkind"),
            direction: row.get("direction"),
            start_bar,
            end_bar,
            start_time: row.get("start_time"),
            end_time: row.get("end_time"),
            invalidated: row.get("invalidated"),
            anchors,
            score: raw_meta.get("score").and_then(|v| v.as_f64()),
            ratios: raw_meta.get("ratios").cloned(),
            extension: raw_meta.get("extension").and_then(|v| v.as_bool()),
        });
    }

    Ok(Json(HarmonicResponse {
        venue,
        symbol,
        timeframe: tf,
        candles,
        patterns,
    }))
}

/// Remap stored anchor JSON into current-window HarmonicAnchors via
/// `time → candle.time` binary search. Returns None if any anchor is
/// outside the window — frontend can't draw a half-off-chart XABCD.
fn remap_anchors(
    anchors_json: &serde_json::Value,
    candles: &[HarmonicCandle],
) -> Option<Vec<HarmonicAnchor>> {
    let arr = anchors_json.as_array()?;
    let mut out: Vec<HarmonicAnchor> = Vec::with_capacity(arr.len());
    for item in arr {
        let time: DateTime<Utc> = item
            .get("time")
            .and_then(|t| serde_json::from_value(t.clone()).ok())?;
        let price = item.get("price").and_then(|p| p.as_f64())?;
        let label = item
            .get("label_override")
            .and_then(|l| l.as_str())
            .unwrap_or("")
            .to_string();
        let idx = candles.binary_search_by(|c| c.time.cmp(&time)).ok()?;
        out.push(HarmonicAnchor {
            bar_index: idx as i64,
            time,
            price,
            label,
        });
    }
    Some(out)
}
