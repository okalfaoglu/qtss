#![allow(dead_code)]
//! `GET /v2/regime/{venue}/{symbol}/{tf}` -- Faz 5 Adim (d).
//!
//! Streams recent bars through `qtss_regime::RegimeEngine` and returns
//! the latest classification plus a short history strip for the HUD.
//!
//! The classifier only consumes OHLC + volume, so the surrounding
//! `Instrument` is built as a transport-only placeholder -- the engine
//! never inspects venue/asset_class/session here.

use axum::extract::{Path, Query, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::regime::{RegimeKind, RegimeSnapshot};
use qtss_domain::v2::timeframe::Timeframe;
use qtss_gui_api::{
    RegimeDashboard, RegimeDashboardEntry, RegimeHeatmap, RegimeHeatmapCell,
    RegimeHud, RegimeIntervalEntry, RegimeParamOverrideView, RegimePoint,
    RegimeTimeline, RegimeTimelinePoint, RegimeTransitionView, RegimeView,
};
use qtss_regime::{RegimeConfig, RegimeEngine};
use qtss_storage::{market_bars, regime_snapshots, regime_transitions, regime_param_overrides, regime_performance};

use crate::error::ApiError;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct RegimeQuery {
    /// How many recent bars to feed the engine.
    pub window: Option<i64>,
    /// How many recent classifications to keep in the history strip.
    pub history: Option<usize>,
    pub segment: Option<String>,
}

pub fn v2_regime_router() -> Router<SharedState> {
    Router::new()
        .route("/v2/regime/{venue}/{symbol}/{tf}", get(get_regime))
        .route("/v2/regime/dashboard", get(get_dashboard))
        .route("/v2/regime/heatmap", get(get_heatmap))
        .route("/v2/regime/transitions", get(get_transitions))
        .route("/v2/regime/timeline/{symbol}/{interval}", get(get_timeline))
        .route("/v2/regime/params/{regime}", get(get_params))
        .route("/v2/regime/params/{regime}", put(put_params))
        .route("/v2/regime/performance", get(get_performance))
}

async fn get_regime(
    State(st): State<SharedState>,
    Path((venue, symbol, tf)): Path<(String, String, String)>,
    Query(q): Query<RegimeQuery>,
) -> Result<Json<RegimeHud>, ApiError> {
    let segment = q.segment.unwrap_or_else(|| "spot".to_string());
    let window = q
        .window
        .unwrap_or_else(|| env_int("QTSS_V2_REGIME_WINDOW", 400))
        .clamp(50, 5_000);
    let history_len = q
        .history
        .unwrap_or_else(|| env_int("QTSS_V2_REGIME_HISTORY", 60) as usize)
        .clamp(1, 1_000);

    let rows =
        market_bars::list_recent_bars(&st.pool, &venue, &segment, &symbol, &tf, window).await?;

    // DB returns newest-first; engine needs chronological order.
    let mut rows = rows;
    rows.reverse();

    let timeframe = parse_timeframe(&tf)
        .ok_or_else(|| ApiError::bad_request(format!("invalid timeframe: {tf}")))?;
    let instrument = placeholder_instrument(&venue, &symbol);

    let mut engine = RegimeEngine::new(RegimeConfig::defaults())
        .map_err(|e| ApiError::internal(format!("regime engine init: {e}")))?;
    let mut history: Vec<RegimeSnapshot> = Vec::new();

    for r in rows {
        let bar = Bar {
            instrument: instrument.clone(),
            timeframe,
            open_time: r.open_time,
            open: r.open,
            high: r.high,
            low: r.low,
            close: r.close,
            volume: r.volume,
            closed: true,
        };
        if let Some(snap) = engine
            .on_bar(&bar)
            .map_err(|e| ApiError::internal(format!("regime on_bar: {e}")))?
        {
            history.push(snap);
        }
    }

    let current = history.last().cloned().map(RegimeView::from);
    let strip_start = history.len().saturating_sub(history_len);
    let strip: Vec<RegimePoint> = history[strip_start..].iter().map(RegimePoint::from).collect();

    Ok(Json(RegimeHud {
        generated_at: chrono::Utc::now(),
        venue,
        symbol,
        timeframe: tf,
        current,
        history: strip,
    }))
}

// =========================================================================
// Faz 11 — new endpoints
// =========================================================================

async fn get_dashboard(
    State(st): State<SharedState>,
) -> Result<Json<RegimeDashboard>, ApiError> {
    let rows = regime_snapshots::latest_snapshots_all(&st.pool).await?;

    // Group by symbol
    let mut by_symbol: HashMap<String, Vec<&regime_snapshots::RegimeSnapshotRow>> = HashMap::new();
    for r in &rows {
        by_symbol.entry(r.symbol.clone()).or_default().push(r);
    }

    let tf_weights_str = qtss_storage::resolve_system_string(
        &st.pool, "regime", "tf_weights", "", r#"{"5m":0.1,"15m":0.15,"1h":0.25,"4h":0.30,"1d":0.20}"#,
    ).await;
    let tf_weights: HashMap<String, f64> = serde_json::from_str(&tf_weights_str)
        .unwrap_or_else(|_| qtss_regime::multi_tf::default_tf_weights());

    let mut entries = Vec::new();
    for (symbol, snap_rows) in &by_symbol {
        let intervals: Vec<RegimeIntervalEntry> = snap_rows.iter().map(|r| {
            RegimeIntervalEntry {
                interval: r.interval.clone(),
                regime: RegimeKind::from_str_opt(&r.regime).unwrap_or(RegimeKind::Uncertain),
                confidence: r.confidence as f32,
            }
        }).collect();

        // Build snapshots for multi-TF computation
        let snap_pairs: Vec<(String, RegimeSnapshot)> = snap_rows.iter().filter_map(|r| {
            Some((r.interval.clone(), row_to_snapshot(r)?))
        }).collect();

        let mtf = qtss_regime::multi_tf::compute_confluence(symbol, &snap_pairs, &tf_weights);
        let (dominant, score, transitioning) = match mtf {
            Some(m) => (m.dominant_regime, m.confluence_score, m.is_transitioning),
            None => (RegimeKind::Uncertain, 0.0, false),
        };

        entries.push(RegimeDashboardEntry {
            symbol: symbol.clone(),
            intervals,
            dominant_regime: dominant,
            confluence_score: score,
            is_transitioning: transitioning,
        });
    }
    entries.sort_by(|a, b| a.symbol.cmp(&b.symbol));

    Ok(Json(RegimeDashboard {
        generated_at: chrono::Utc::now(),
        entries,
    }))
}

async fn get_heatmap(
    State(st): State<SharedState>,
) -> Result<Json<RegimeHeatmap>, ApiError> {
    let rows = regime_snapshots::latest_snapshots_all(&st.pool).await?;

    let mut symbols_set = std::collections::BTreeSet::new();
    let mut intervals_set = std::collections::BTreeSet::new();
    let mut cells = Vec::new();

    for r in &rows {
        symbols_set.insert(r.symbol.clone());
        intervals_set.insert(r.interval.clone());
        cells.push(RegimeHeatmapCell {
            symbol: r.symbol.clone(),
            interval: r.interval.clone(),
            regime: RegimeKind::from_str_opt(&r.regime).unwrap_or(RegimeKind::Uncertain),
            confidence: r.confidence as f32,
        });
    }

    Ok(Json(RegimeHeatmap {
        generated_at: chrono::Utc::now(),
        symbols: symbols_set.into_iter().collect(),
        intervals: intervals_set.into_iter().collect(),
        cells,
    }))
}

async fn get_transitions(
    State(st): State<SharedState>,
    Query(q): Query<TransitionQuery>,
) -> Result<Json<Vec<RegimeTransitionView>>, ApiError> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let rows = if q.active_only.unwrap_or(false) {
        regime_transitions::list_active_transitions(&st.pool).await?
    } else {
        regime_transitions::list_recent_transitions(&st.pool, limit as i64).await?
    };

    let views: Vec<RegimeTransitionView> = rows.iter().map(|r| {
        RegimeTransitionView {
            id: r.id.to_string(),
            symbol: r.symbol.clone(),
            interval: r.interval.clone(),
            from_regime: r.from_regime.clone(),
            to_regime: r.to_regime.clone(),
            transition_speed: r.transition_speed,
            confidence: r.confidence,
            confirming_indicators: r.confirming_indicators.0.clone(),
            detected_at: r.detected_at,
            resolved_at: r.resolved_at,
            was_correct: r.was_correct,
        }
    }).collect();

    Ok(Json(views))
}

#[derive(Debug, Deserialize)]
struct TransitionQuery {
    limit: Option<usize>,
    active_only: Option<bool>,
}

async fn get_timeline(
    State(st): State<SharedState>,
    Path((symbol, interval)): Path<(String, String)>,
    Query(q): Query<TimelineQuery>,
) -> Result<Json<RegimeTimeline>, ApiError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 2000) as i64;
    let mut rows = regime_snapshots::regime_timeline(&st.pool, &symbol, &interval, limit).await?;
    rows.reverse(); // oldest first for timeline

    let points: Vec<RegimeTimelinePoint> = rows.iter().map(|r| {
        RegimeTimelinePoint {
            at: r.computed_at,
            regime: r.regime.clone(),
            confidence: r.confidence,
        }
    }).collect();

    Ok(Json(RegimeTimeline { symbol, interval, points }))
}

#[derive(Debug, Deserialize)]
struct TimelineQuery {
    limit: Option<usize>,
}

async fn get_params(
    State(st): State<SharedState>,
    Path(regime): Path<String>,
) -> Result<Json<Vec<RegimeParamOverrideView>>, ApiError> {
    let rows = regime_param_overrides::list_overrides_for_regime(&st.pool, &regime).await?;
    let views: Vec<RegimeParamOverrideView> = rows.iter().map(|r| {
        RegimeParamOverrideView {
            module: r.module.clone(),
            config_key: r.config_key.clone(),
            regime: r.regime.clone(),
            value: r.value.0.clone(),
            description: r.description.clone(),
        }
    }).collect();
    Ok(Json(views))
}

#[derive(Debug, Deserialize)]
struct ParamUpdate {
    module: String,
    config_key: String,
    value: serde_json::Value,
    description: Option<String>,
}

async fn put_params(
    State(st): State<SharedState>,
    Path(regime): Path<String>,
    Json(body): Json<Vec<ParamUpdate>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut updated = 0u32;
    for p in &body {
        regime_param_overrides::upsert_override(
            &st.pool,
            &p.module,
            &p.config_key,
            &regime,
            p.value.clone(),
            p.description.as_deref(),
        ).await?;
        updated += 1;
    }
    Ok(Json(serde_json::json!({ "updated": updated })))
}

#[derive(Debug, Deserialize)]
struct PerformanceQuery {
    days: Option<i64>,
}

async fn get_performance(
    State(st): State<SharedState>,
    Query(q): Query<PerformanceQuery>,
) -> Result<Json<Vec<regime_performance::RegimePerformanceRow>>, ApiError> {
    let days = q.days.unwrap_or(30).clamp(1, 365);
    let rows = regime_performance::regime_performance(&st.pool, days).await?;
    Ok(Json(rows))
}

/// Convert a DB row into a domain RegimeSnapshot (for multi-TF computation).
fn row_to_snapshot(r: &regime_snapshots::RegimeSnapshotRow) -> Option<RegimeSnapshot> {
    use rust_decimal::Decimal;
    Some(RegimeSnapshot {
        at: r.computed_at,
        kind: RegimeKind::from_str_opt(&r.regime)?,
        trend_strength: r.trend_strength.as_deref()
            .and_then(qtss_domain::v2::regime::TrendStrength::from_str_opt)
            .unwrap_or(qtss_domain::v2::regime::TrendStrength::None),
        adx: Decimal::from_f64_retain(r.adx.unwrap_or(0.0)).unwrap_or_default(),
        bb_width: Decimal::from_f64_retain(r.bb_width.unwrap_or(0.0)).unwrap_or_default(),
        atr_pct: Decimal::from_f64_retain(r.atr_pct.unwrap_or(0.0)).unwrap_or_default(),
        choppiness: Decimal::from_f64_retain(r.choppiness.unwrap_or(0.0)).unwrap_or_default(),
        confidence: r.confidence as f32,
    })
}

/// Transport-only instrument; the regime classifier consumes OHLCV
/// only and never inspects these fields.
fn placeholder_instrument(venue: &str, symbol: &str) -> Instrument {
    let v = match venue.to_lowercase().as_str() {
        "binance" => Venue::Binance,
        other => Venue::Custom(other.to_string()),
    };
    Instrument {
        venue: v,
        asset_class: AssetClass::CryptoSpot,
        symbol: symbol.to_string(),
        quote_ccy: "USDT".to_string(),
        tick_size: Decimal::new(1, 8),
        lot_size: Decimal::new(1, 8),
        session: SessionCalendar::binance_24x7(),
    }
}

/// `market_bars` interval strings ("1m", "4h", "1d") → `Timeframe`.
/// `Timeframe::FromStr` only accepts the lowercase Debug form
/// ("m1", "h4") so the chart/regime endpoints need this translator.
/// Mirror of the helper in `qtss-worker::v2_detection_orchestrator`
/// — kept inline here to avoid pulling the worker crate into the API.
fn parse_timeframe(interval: &str) -> Option<Timeframe> {
    match interval.trim().to_lowercase().as_str() {
        "1m" => Some(Timeframe::M1),
        "3m" => Some(Timeframe::M3),
        "5m" => Some(Timeframe::M5),
        "15m" => Some(Timeframe::M15),
        "30m" => Some(Timeframe::M30),
        "1h" => Some(Timeframe::H1),
        "2h" => Some(Timeframe::H2),
        "4h" => Some(Timeframe::H4),
        "6h" => Some(Timeframe::H6),
        "8h" => Some(Timeframe::H8),
        "12h" => Some(Timeframe::H12),
        "1d" => Some(Timeframe::D1),
        "3d" => Some(Timeframe::D3),
        "1w" => Some(Timeframe::W1),
        "1mo" | "1mn" => Some(Timeframe::Mn1),
        _ => None,
    }
}

fn env_int(key: &str, default: i64) -> i64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parses_window_and_history() {
        let q: RegimeQuery = serde_urlencoded::from_str("window=300&history=20").unwrap();
        assert_eq!(q.window, Some(300));
        assert_eq!(q.history, Some(20));
    }

    #[test]
    fn placeholder_instrument_uses_custom_venue() {
        let i = placeholder_instrument("dydx", "ETHUSD");
        assert_eq!(i.symbol, "ETHUSD");
        match i.venue {
            Venue::Custom(s) => assert_eq!(s, "dydx"),
            _ => panic!("expected Custom venue"),
        }
    }
}
