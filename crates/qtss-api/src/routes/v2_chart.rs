#![allow(dead_code)]
//! `GET /v2/chart/{venue}/{symbol}/{tf}` -- Faz 5 Adim (b).
//!
//! Single round-trip chart workspace payload: candles + renko bricks +
//! pattern overlays + open positions + open orders. Designed so the
//! React chart panel can flip between candle and renko views without
//! refetching.
//!
//! ## Data sources today
//!
//! - **Candles**: `qtss_storage::market_bars::list_recent_bars` --
//!   the canonical OHLCV table. Segment defaults to `spot` (override
//!   via `?segment=`).
//! - **Renko**: `qtss_gui_api::build_renko` over the same candles.
//!   Brick size is resolved from `system_config`
//!   (`api.v2_chart_renko_brick_pct`) -- nothing hardcoded
//!   (CLAUDE.md #2). Frontend can override per request via
//!   `?brick_pct=` query for ad-hoc experimentation.
//! - **Positions**: from the in-memory `V2DashboardHandle` engine
//!   (the same one `/v2/dashboard` reads), filtered to the symbol.
//! - **Detections** + **Open orders**: stubbed empty for now -- the
//!   v2 detection registry and v2 open-order book do not exist yet.
//!   The wire shape is in place so adding them is a one-line splice.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::Deserialize;

use qtss_gui_api::{
    build_renko, CandleBar, ChartWorkspace, DetectionAnchor, DetectionOverlay, OpenOrderOverlay,
    OpenPositionView,
};
use qtss_storage::market_bars;
use qtss_storage::market_bars_open;
use qtss_storage::wave_chain;

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct ChartQuery {
    /// Number of candles to return (newest first from DB, then
    /// reversed to chronological for the wire).
    pub limit: Option<i64>,
    /// Override the configured renko brick percentage. Useful for
    /// quick visual experiments without touching `system_config`.
    pub brick_pct: Option<Decimal>,
    /// Defaults to `spot` -- the only segment v2 wires today.
    pub segment: Option<String>,
    /// Pan-left cursor: when set, return `limit` bars whose
    /// `open_time < before`. The frontend passes its current oldest
    /// candle's `open_time` to walk back through history.
    pub before: Option<chrono::DateTime<chrono::Utc>>,
    /// Faz 12 — which detection modes to overlay. Accepts a
    /// comma-separated list of `live,dry,backtest`. Defaults to
    /// `live,dry` (legacy behaviour).
    pub modes: Option<String>,
    /// Faz 12 — CSV of pivot levels (`L0,L1,L2,L3`) the frontend
    /// wants to see. Filters backtest overlays to the levels whose
    /// toggle button is ON. Defaults to all four.
    pub levels: Option<String>,
    /// BUG5 — Append the live forming bar from `market_bars_open`
    /// when this is the current view (no `before` cursor). Defaults
    /// to `true` so the chart matches the zigzag/elliott/harmonic
    /// overlays which already merge the open bar; setting this to
    /// `false` returns only closed bars (e.g. for analysis snapshots
    /// that must not see the still-forming candle).
    pub include_open: Option<bool>,
}

pub fn v2_chart_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/chart/{venue}/{symbol}/{tf}", get(get_chart))
        // Faz 9.8.x — chart toolbar combobox source: distinct
        // (exchange, segment) pairs that are enabled in engine_symbols,
        // each with their symbol + interval list. Nothing hardcoded
        // on the frontend (CLAUDE.md #2).
        .route("/v2/chart/venues", get(list_chart_venues))
}

#[derive(Debug, serde::Serialize)]
struct ChartVenueOption {
    exchange: String,
    segment: String,
    symbols: Vec<String>,
    intervals: Vec<String>,
    /// Faz 13.UI — per-symbol interval lookup. Keyed by symbol, lists
    /// which timeframes are `enabled=true` for that specific symbol.
    /// GUI uses this to gray out timeframe buttons that have no data
    /// for the currently selected symbol.
    symbol_intervals: std::collections::BTreeMap<String, Vec<String>>,
}

async fn list_chart_venues(
    State(st): State<SharedState>,
) -> Result<Json<Vec<ChartVenueOption>>, ApiError> {
    // One round-trip that the frontend can group by (exchange, segment)
    // into chained dropdowns. `enabled = true` filters out discovery
    // ghosts that the operator has not confirmed.
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        r#"
        SELECT exchange, segment, symbol, "interval"
          FROM engine_symbols
         WHERE enabled = true
         ORDER BY exchange, segment, sort_order, symbol, "interval"
        "#,
    )
    .fetch_all(&st.pool)
    .await
    .map_err(|e| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    use std::collections::BTreeMap;
    #[derive(Default)]
    struct Acc {
        symbols: Vec<String>,
        intervals: Vec<String>,
        per_symbol: BTreeMap<String, Vec<String>>,
    }
    let mut grouped: BTreeMap<(String, String), Acc> = BTreeMap::new();
    for (exchange, segment, symbol, interval) in rows {
        let entry = grouped.entry((exchange, segment)).or_default();
        if !entry.symbols.contains(&symbol) {
            entry.symbols.push(symbol.clone());
        }
        if !entry.intervals.contains(&interval) {
            entry.intervals.push(interval.clone());
        }
        let per = entry.per_symbol.entry(symbol).or_default();
        if !per.contains(&interval) {
            per.push(interval);
        }
    }

    Ok(Json(
        grouped
            .into_iter()
            .map(|((exchange, segment), acc)| ChartVenueOption {
                exchange,
                segment,
                symbols: acc.symbols,
                intervals: acc.intervals,
                symbol_intervals: acc.per_symbol,
            })
            .collect(),
    ))
}

async fn get_chart(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<ChartQuery>,
) -> Result<Json<ChartWorkspace>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "futures".to_string());
    let limit = q.limit.unwrap_or(500).clamp(1, 5_000);

    let rows = match q.before {
        Some(before) => {
            market_bars::list_recent_bars_before(
                &st.pool, &venue, &segment, &symbol, &tf, before, limit,
            )
            .await?
        }
        None => {
            market_bars::list_recent_bars(&st.pool, &venue, &segment, &symbol, &tf, limit).await?
        }
    };

    // DB returns newest-first; wire needs chronological for renko.
    let mut candles: Vec<CandleBar> = rows
        .into_iter()
        .map(|r| CandleBar {
            open_time: r.open_time,
            open: r.open,
            high: r.high,
            low: r.low,
            close: r.close,
            volume: r.volume,
        })
        .collect();
    candles.reverse();

    // BUG5 — Append the live forming bar so chart candles stay in
    // lockstep with the zigzag / elliott / harmonic overlays (which
    // already merge `market_bars_open`). Without this the operator
    // sees those overlays drawing on a "ghost" candle that the price
    // axis hasn't drawn yet — looks like a lag and on short TFs causes
    // late entries that close at a loss. Skip when paging back into
    // history (`before` set) or when caller explicitly opts out.
    let include_open = q.before.is_none() && q.include_open.unwrap_or(true);
    if include_open {
        if let Ok(Some(open_bar)) =
            market_bars_open::get_open_bar(&st.pool, &venue, &segment, &symbol, &tf).await
        {
            let is_newer = candles
                .last()
                .map(|c| open_bar.open_time > c.open_time)
                .unwrap_or(true);
            if is_newer {
                candles.push(CandleBar {
                    open_time: open_bar.open_time,
                    open: open_bar.open,
                    high: open_bar.high,
                    low: open_bar.low,
                    close: open_bar.close,
                    volume: open_bar.volume,
                });
            }
        }
    }

    let brick_pct = match q.brick_pct {
        Some(p) => p,
        None => resolve_brick_pct(&st).await,
    };
    let brick_size = match candles.last() {
        Some(last) => last.close * brick_pct,
        None => Decimal::ZERO,
    };
    let renko = build_renko(&candles, brick_size);

    let positions = positions_for(&st, &symbol).await;
    let modes = parse_csv_lower(q.modes.as_deref(), &["live", "dry"]);
    let levels = parse_csv_upper(q.levels.as_deref(), &["L0", "L1", "L2", "L3"]);
    let detections = detections_for(&st, &venue, &segment, &symbol, &tf, &modes, &levels).await;
    let open_orders: Vec<OpenOrderOverlay> = Vec::new();

    Ok(Json(ChartWorkspace {
        generated_at: chrono::Utc::now(),
        venue,
        symbol,
        timeframe: tf,
        candles,
        renko,
        detections,
        positions,
        open_orders,
    }))
}

/// Pull the renko brick percentage from `system_config`. Falls back
/// to a tiny conservative default only when the row is missing AND
/// the env var is unset -- not a "magic constant" but the
/// bootstrap-time fallback that the operator can override.
async fn resolve_brick_pct(st: &SharedState) -> Decimal {
    let raw = qtss_storage::resolve_system_string(
        &st.pool,
        "api",
        "v2_chart_renko_brick_pct",
        "QTSS_V2_CHART_RENKO_BRICK_PCT",
        "0.005",
    )
    .await;
    raw.parse::<Decimal>().unwrap_or_else(|_| Decimal::new(5, 3))
}

/// Read pattern overlays from the new `detections` table (populated by
/// `qtss-engine`'s elliott / harmonic writers). Honours the frontend's
/// `modes` (live/dry/backtest) and `levels` (L0–L3 → slot 0–3) toggles.
/// Invalidated rows are filtered out so the chart shows only active
/// geometry; backtest evaluation results would fan in as a separate
/// join once the outcome table is wired up.
async fn detections_for(
    st: &SharedState,
    venue: &str,
    segment: &str,
    symbol: &str,
    tf: &str,
    modes: &[String],
    levels: &[String],
) -> Vec<DetectionOverlay> {
    let slot_ints: Vec<i16> = levels
        .iter()
        .filter_map(|l| l.strip_prefix('L').and_then(|n| n.parse::<i16>().ok()))
        .collect();
    if slot_ints.is_empty() || modes.is_empty() {
        return Vec::new();
    }

    // Fair-share across families: we let the DB rank by `start_time`
    // first so the freshest signals of every family (SMC, classical,
    // range, etc.) make the cut. With `ORDER BY slot` first, slot-0
    // families (classical, range, gap, candle, orb) could saturate
    // the 2000-row budget and starve slot-1+ families like SMC.
    //
    // FAZ 25.4.A — symbol-level families bypass the Z-slot filter:
    //   gap / candle: price+volume mechanics on raw bar tape
    //   wyckoff events + ranges: stored at slot=0 (per-symbol annotation)
    // FAZ 25.4.E — wyckoff CYCLES are now per-slot (0-5) and MUST
    // respect the slot filter so toggling Z5 doesn't paint Z0..Z4
    // distribution boxes on top of each other. Detected via subkind
    // prefix `cycle_*` — events stay at slot=0, cycles at slot=N.
    let rows = match sqlx::query(
        r#"SELECT slot, pattern_family, subkind, direction,
                  start_time, end_time,
                  anchors, live, invalidated, raw_meta, mode
             FROM detections
            WHERE exchange = $1 AND segment = $2 AND symbol = $3
              AND timeframe = $4
              AND (
                  slot = ANY($5)
                  OR pattern_family IN ('gap', 'candle')
                  OR (pattern_family = 'wyckoff'
                      AND subkind NOT LIKE 'cycle_%'
                      AND subkind NOT LIKE 'phase_%')
              )
              AND mode = ANY($6)
              AND invalidated = false
            ORDER BY start_time DESC
            LIMIT 2000"#,
    )
    .bind(venue)
    .bind(segment)
    .bind(symbol)
    .bind(tf)
    .bind(&slot_ints)
    .bind(modes)
    .fetch_all(&st.pool)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(%e, venue, segment, symbol, tf, "detections_for: query failed");
            return Vec::new();
        }
    };

    rows.into_iter()
        .filter_map(|row| row_to_overlay(row))
        .collect()
}

fn row_to_overlay(row: sqlx::postgres::PgRow) -> Option<DetectionOverlay> {
    use sqlx::Row;

    let slot: i16 = row.get("slot");
    let family: String = row.get("pattern_family");
    let subkind: String = row.get("subkind");
    let direction: i16 = row.get("direction");
    let start_time: chrono::DateTime<chrono::Utc> = row.get("start_time");
    let end_time: chrono::DateTime<chrono::Utc> = row.get("end_time");
    let anchors_json: serde_json::Value =
        row.try_get("anchors").unwrap_or(serde_json::Value::Null);
    let live: Option<bool> = row.try_get("live").ok().flatten();
    let invalidated: bool = row.get("invalidated");
    let raw_meta: serde_json::Value =
        row.try_get("raw_meta").unwrap_or(serde_json::Value::Null);
    let mode: String = row.get("mode");

    let anchors = parse_anchors(&anchors_json);
    let first = anchors.first()?.clone();

    let state = match (invalidated, live) {
        (true, _) => "invalidated",
        (false, Some(true)) => "forming",
        (false, _) => "confirmed",
    };

    let confidence = raw_meta
        .get("score")
        .and_then(|v| v.as_f64())
        .and_then(Decimal::from_f64)
        .unwrap_or(Decimal::ZERO);

    let arrow = match direction.signum() {
        1 => " ↑",
        -1 => " ↓",
        _ => "",
    };

    let raw_meta_passthrough = if raw_meta.is_null() {
        None
    } else {
        Some(raw_meta.clone())
    };
    Some(DetectionOverlay {
        id: format!(
            "{}:{}:{}:{}:{}",
            family,
            subkind,
            slot,
            start_time.timestamp_millis(),
            end_time.timestamp_millis(),
        ),
        kind: family.clone(),
        label: format!("{}{}", subkind, arrow),
        family,
        subkind,
        state: state.to_string(),
        anchor_time: first.time,
        anchor_price: first.price,
        confidence,
        invalidation_price: Decimal::ZERO,
        anchors,
        projected_anchors: Vec::new(),
        sub_wave_anchors: Vec::new(),
        wave_context: None,
        has_children: false,
        render_geometry: None,
        render_style: None,
        render_labels: None,
        pivot_level: Some(format!("L{}", slot)),
        mode: Some(mode),
        outcome: None,
        outcome_pnl_pct: None,
        outcome_entry_price: None,
        outcome_exit_price: None,
        outcome_close_reason: None,
        targets: None,
        raw_meta: raw_meta_passthrough,
    })
}

/// Parse a comma-separated query param, lower-cased, falling back to
/// `defaults` when missing/empty.
fn parse_csv_lower(raw: Option<&str>, defaults: &[&str]) -> Vec<String> {
    parse_csv_with(raw, defaults, |s| s.to_ascii_lowercase())
}

/// Parse a comma-separated query param, upper-cased, falling back to
/// `defaults` when missing/empty.
fn parse_csv_upper(raw: Option<&str>, defaults: &[&str]) -> Vec<String> {
    parse_csv_with(raw, defaults, |s| s.to_ascii_uppercase())
}

fn parse_csv_with(raw: Option<&str>, defaults: &[&str], normalize: fn(&str) -> String) -> Vec<String> {
    let parsed: Vec<String> = raw
        .unwrap_or("")
        .split(',')
        .map(|s| normalize(s.trim()))
        .filter(|s| !s.is_empty())
        .collect();
    if parsed.is_empty() {
        defaults.iter().map(|s| normalize(s)).collect()
    } else {
        parsed
    }
}

/// Parse the persisted anchor JSON into the wire shape. The orchestrator
/// writes `anchors` as a `Vec<PivotRef>` (`{ time, price, kind, idx }`)
/// — we keep only what the chart needs (time, price, optional label).
/// Robust to both string and numeric price encodings since the
/// orchestrator's serializer used to flip between them.
fn parse_anchors(value: &serde_json::Value) -> Vec<DetectionAnchor> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            let time = v
                .get("time")
                .and_then(|t| t.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&chrono::Utc))?;
            let price = v.get("price").and_then(|p| {
                if let Some(s) = p.as_str() {
                    s.parse::<Decimal>().ok()
                } else {
                    p.as_f64().and_then(Decimal::from_f64)
                }
            })?;
            let label = v
                .get("label")
                .and_then(|k| k.as_str())
                .map(|s| s.to_string())
                .or_else(|| v.get("label_override").and_then(|k| k.as_str()).map(|s| s.to_string()))
                .or_else(|| v.get("kind").and_then(|k| k.as_str()).map(|s| s.to_string()));
            Some(DetectionAnchor { time, price, label })
        })
        .collect()
}

async fn positions_for(st: &SharedState, symbol: &str) -> Vec<OpenPositionView> {
    let snap = st.v2_dashboard.snapshot().await;
    snap.open_positions
        .into_iter()
        .filter(|p| p.symbol == symbol)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn brick_pct_query_overrides_config() {
        // Smoke: just confirm the parser path -- the route handler
        // itself needs an HTTP harness, which we cover at the
        // integration tier.
        let q: ChartQuery = serde_urlencoded::from_str("brick_pct=0.01&limit=100").unwrap();
        assert_eq!(q.brick_pct, Some(dec!(0.01)));
        assert_eq!(q.limit, Some(100));
    }
}
