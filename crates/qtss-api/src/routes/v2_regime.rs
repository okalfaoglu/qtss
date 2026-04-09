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
use axum::routing::get;
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::Deserialize;
use qtss_domain::v2::bar::Bar;
use qtss_domain::v2::instrument::{AssetClass, Instrument, SessionCalendar, Venue};
use qtss_domain::v2::regime::RegimeSnapshot;
use qtss_domain::v2::timeframe::Timeframe;
use qtss_gui_api::{RegimeHud, RegimePoint, RegimeView};
use qtss_regime::{RegimeConfig, RegimeEngine};
use qtss_storage::market_bars;

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
    Router::new().route("/v2/regime/{venue}/{symbol}/{tf}", get(get_regime))
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
