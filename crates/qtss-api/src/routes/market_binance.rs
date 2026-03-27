//! Halka açık Binance OHLCV ve komisyon özetleri (okuma).

use std::str::FromStr;

use axum::extract::{Extension, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{TimeZone, Utc};
use serde::Deserialize;
use serde_json::json;

use qtss_binance::{
    default_spot_commission_bps, default_usdt_futures_commission_bps,
    futures_commission_hint_from_exchange_info, parse_klines_json, public_spot_kline_url,
    public_usdm_kline_url, spot_commission_hint_from_exchange_info, BinanceClient, BinanceClientConfig,
    CommissionBps, KlineBar,
};
use qtss_storage::{list_recent_bars, upsert_market_bar, MarketBarUpsert};
use rust_decimal::Decimal;

use crate::oauth::AccessClaims;
use crate::state::SharedState;

#[derive(Deserialize)]
pub struct KlinesQuery {
    pub symbol: String,
    pub interval: String,
    pub segment: Option<String>,
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    pub limit: Option<u32>,
}

#[derive(Deserialize)]
pub struct CommissionQuery {
    pub segment: Option<String>,
    /// Doluysa ilgili `exchangeInfo` çekilir; sembol satırında ücret alanları varsa kullanılır.
    pub symbol: Option<String>,
}

#[derive(Deserialize)]
pub struct RecentBarsQuery {
    pub exchange: String,
    pub segment: String,
    pub symbol: String,
    pub interval: String,
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct StreamUrlQuery {
    pub symbol: String,
    pub interval: String,
}

pub fn market_binance_router() -> Router<SharedState> {
    Router::new()
        .route("/market/binance/klines", get(binance_klines))
        .route("/market/binance/commission-defaults", get(binance_commission_defaults))
        .route("/market/binance/stream-urls", get(binance_stream_urls))
        .route("/market/bars/recent", get(market_bars_recent))
}

/// Binance → `market_bars` yazımı (REST backfill). JWT + `admin`/`trader` (`require_ops_roles`).
pub fn market_binance_write_router() -> Router<SharedState> {
    Router::new().route(
        "/market/binance/bars/backfill",
        post(backfill_market_bars_from_rest),
    )
}

#[derive(Deserialize)]
pub struct BackfillBody {
    pub symbol: String,
    pub interval: String,
    pub segment: Option<String>,
    /// Binance üst sınırına yakın (max 1000).
    pub limit: Option<u32>,
}

async fn binance_klines(
    Extension(_claims): Extension<AccessClaims>,
    State(_st): State<SharedState>,
    Query(q): Query<KlinesQuery>,
) -> Result<Json<Vec<KlineBar>>, String> {
    let cfg = BinanceClientConfig::public_mainnet();
    let client = BinanceClient::new(cfg).map_err(|e| e.to_string())?;
    let seg = q.segment.as_deref().unwrap_or("spot");
    let raw = match seg {
        "futures" | "usdt_futures" | "fapi" => {
            client
                .fapi_klines(
                    &q.symbol,
                    &q.interval,
                    q.start_time,
                    q.end_time,
                    q.limit,
                )
                .await
                .map_err(|e| e.to_string())?
        }
        _ => {
            client
                .spot_klines(
                    &q.symbol,
                    &q.interval,
                    q.start_time,
                    q.end_time,
                    q.limit,
                )
                .await
                .map_err(|e| e.to_string())?
        }
    };
    let bars = parse_klines_json(&raw).map_err(|e| e.to_string())?;
    Ok(Json(bars))
}

async fn binance_commission_defaults(
    Extension(_claims): Extension<AccessClaims>,
    Query(q): Query<CommissionQuery>,
) -> Result<Json<serde_json::Value>, String> {
    let seg = q.segment.as_deref().unwrap_or("spot");
    let sym_upper = q
        .symbol
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_uppercase());

    let mut from_info: Option<CommissionBps> = None;
    let mut source = "fallback_tier0";

    if let Some(ref sym) = sym_upper {
        let cfg = BinanceClientConfig::public_mainnet();
        let client = BinanceClient::new(cfg).map_err(|e| e.to_string())?;
        let raw = match seg {
            "futures" | "usdt_futures" | "fapi" => client
                .fapi_exchange_info(Some(sym))
                .await
                .map_err(|e| e.to_string())?,
            _ => client
                .spot_exchange_info(Some(sym))
                .await
                .map_err(|e| e.to_string())?,
        };
        from_info = match seg {
            "futures" | "usdt_futures" | "fapi" => {
                futures_commission_hint_from_exchange_info(&raw, sym)
            }
            _ => spot_commission_hint_from_exchange_info(&raw, sym),
        };
        if from_info.is_some() {
            source = "exchange_info";
        }
    }

    let defaults: CommissionBps = from_info.unwrap_or_else(|| match seg {
        "futures" | "usdt_futures" | "fapi" => default_usdt_futures_commission_bps(),
        _ => default_spot_commission_bps(),
    });
    Ok(Json(json!({
        "segment": seg,
        "query_symbol": sym_upper,
        "defaults_bps": defaults,
        "source": source
    })))
}

async fn backfill_market_bars_from_rest(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<BackfillBody>,
) -> Result<Json<serde_json::Value>, String> {
    let sym = body.symbol.trim().to_uppercase();
    if sym.is_empty() {
        return Err("symbol gerekli".into());
    }
    let interval = body.interval.trim().to_string();
    if interval.is_empty() {
        return Err("interval gerekli".into());
    }
    let seg = body.segment.as_deref().unwrap_or("spot");
    let seg_db = match seg {
        "futures" | "usdt_futures" | "fapi" => "futures",
        _ => "spot",
    };
    let lim = body.limit.unwrap_or(500).clamp(1, 1_000);

    let cfg = BinanceClientConfig::public_mainnet();
    let client = BinanceClient::new(cfg).map_err(|e| e.to_string())?;
    let raw = match seg {
        "futures" | "usdt_futures" | "fapi" => client
            .fapi_klines(&sym, &interval, None, None, Some(lim))
            .await
            .map_err(|e| e.to_string())?,
        _ => client
            .spot_klines(&sym, &interval, None, None, Some(lim))
            .await
            .map_err(|e| e.to_string())?,
    };
    let klines = parse_klines_json(&raw).map_err(|e| e.to_string())?;

    let mut upserted = 0_i64;
    for b in &klines {
        let open_time = Utc
            .timestamp_millis_opt(b.open_time as i64)
            .single()
            .ok_or_else(|| format!("open_time geçersiz: {}", b.open_time))?;
        let quote_volume = if b.quote_asset_volume.trim().is_empty() {
            None
        } else {
            Some(
                Decimal::from_str(b.quote_asset_volume.trim()).map_err(|e| e.to_string())?,
            )
        };
        let row = MarketBarUpsert {
            exchange: "binance".into(),
            segment: seg_db.into(),
            symbol: sym.clone(),
            interval: interval.clone(),
            open_time,
            open: Decimal::from_str(b.open.trim()).map_err(|e| e.to_string())?,
            high: Decimal::from_str(b.high.trim()).map_err(|e| e.to_string())?,
            low: Decimal::from_str(b.low.trim()).map_err(|e| e.to_string())?,
            close: Decimal::from_str(b.close.trim()).map_err(|e| e.to_string())?,
            volume: Decimal::from_str(b.volume.trim()).map_err(|e| e.to_string())?,
            quote_volume,
            trade_count: Some(b.number_of_trades as i64),
        };
        upsert_market_bar(&st.pool, &row)
            .await
            .map_err(|e| e.to_string())?;
        upserted += 1;
    }

    Ok(Json(json!({
        "exchange": "binance",
        "segment": seg_db,
        "symbol": sym,
        "interval": interval,
        "upserted": upserted,
        "source": "binance_rest_klines"
    })))
}

async fn market_bars_recent(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<RecentBarsQuery>,
) -> Result<Json<Vec<qtss_storage::MarketBarRow>>, String> {
    let limit = q.limit.unwrap_or(500).clamp(1, 5_000);
    let bars = list_recent_bars(
        &st.pool,
        q.exchange.trim(),
        q.segment.trim(),
        q.symbol.trim(),
        q.interval.trim(),
        limit,
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(Json(bars))
}

async fn binance_stream_urls(
    Extension(_claims): Extension<AccessClaims>,
    Query(q): Query<StreamUrlQuery>,
) -> Json<serde_json::Value> {
    let spot = public_spot_kline_url(&q.symbol, &q.interval);
    let usdm = public_usdm_kline_url(&q.symbol, &q.interval);
    Json(json!({
        "spot_kline_wss": spot,
        "usdm_kline_wss": usdm,
        "note": "İstemci doğrudan bağlanabilir; tarayıcı CORS gerektirmez."
    }))
}
