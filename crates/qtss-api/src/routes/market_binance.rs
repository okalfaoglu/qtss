//! Halka açık Binance OHLCV ve komisyon özetleri (okuma).

use axum::extract::{Extension, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tracing::debug;
use uuid::Uuid;

use qtss_binance::{
    backfill_binance_public_klines, commission_rate_from_fapi_response, default_spot_commission_bps,
    default_usdt_futures_commission_bps, futures_commission_hint_from_exchange_info,
    parse_klines_json, public_spot_kline_url, public_usdm_kline_url,
    spot_commission_hint_from_exchange_info, trade_fee_from_sapi_response, BinanceClient,
    BinanceClientConfig, BinanceError, CommissionBps, KlineBar,
};
use qtss_storage::list_recent_bars;

use crate::error::ApiError;
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
pub struct CommissionAccountQuery {
    pub symbol: String,
    pub segment: Option<String>,
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
        .route(
            "/market/binance/commission-defaults",
            get(binance_commission_defaults),
        )
        .route(
            "/market/binance/commission-account",
            get(binance_commission_account),
        )
        .route("/market/binance/stream-urls", get(binance_stream_urls))
        .route("/market/bars/recent", get(market_bars_recent))
}

/// Binance → `market_bars` yazımı (REST backfill). JWT + `admin`/`trader` (`require_ops_roles`).
pub fn market_binance_write_router() -> Router<SharedState> {
    Router::new()
        .route(
            "/market/binance/bars/backfill",
            post(backfill_market_bars_from_rest),
        )
        .route(
            "/market/binance/futures/leverage",
            post(binance_futures_leverage),
        )
}

#[derive(Deserialize)]
pub struct BackfillBody {
    pub symbol: String,
    pub interval: String,
    pub segment: Option<String>,
    /// İstenen toplam mum; Binance tek istekte en fazla 1000 — sunucu geriye doğru sayfalar (üst sınır 50_000).
    pub limit: Option<u32>,
}

#[derive(Deserialize)]
pub struct FuturesLeverageBody {
    pub symbol: String,
    /// Borsa sembolüne göre üst sınır değişir; istek 1..=125 arası kısıtlanır.
    pub leverage: u32,
}

async fn binance_klines(
    Extension(_claims): Extension<AccessClaims>,
    State(_st): State<SharedState>,
    Query(q): Query<KlinesQuery>,
) -> Result<Json<Vec<KlineBar>>, ApiError> {
    let cfg = BinanceClientConfig::public_mainnet();
    let client = BinanceClient::new(cfg).map_err(|e| ApiError::internal(e.to_string()))?;
    let seg = q.segment.as_deref().unwrap_or("spot").trim().to_lowercase();
    let upstream = match seg.as_str() {
        "future" | "futures" | "usdt_futures" | "fapi" => "fapi",
        _ => "spot_then_maybe_fapi",
    };
    debug!(
        target: "qtss_api::market_binance",
        symbol = %q.symbol,
        interval = %q.interval,
        segment = %seg,
        upstream,
        "binance klines"
    );
    let raw = match seg.as_str() {
        "future" | "futures" | "usdt_futures" | "fapi" => client
            .fapi_klines(&q.symbol, &q.interval, q.start_time, q.end_time, q.limit)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?,
        _ => {
            let spot_res = client
                .spot_klines(&q.symbol, &q.interval, q.start_time, q.end_time, q.limit)
                .await;
            match spot_res {
                Ok(v) => v,
                Err(BinanceError::Api { code: -1121, .. }) => client
                    .fapi_klines(&q.symbol, &q.interval, q.start_time, q.end_time, q.limit)
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?,
                Err(e) => return Err(ApiError::internal(e.to_string())),
            }
        }
    };
    let bars = parse_klines_json(&raw).map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(bars))
}

async fn binance_commission_defaults(
    Extension(_claims): Extension<AccessClaims>,
    Query(q): Query<CommissionQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
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
        let client = BinanceClient::new(cfg).map_err(|e| ApiError::internal(e.to_string()))?;
        let raw = match seg {
            "future" | "futures" | "usdt_futures" | "fapi" => client
                .fapi_exchange_info(Some(sym))
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?,
            _ => client
                .spot_exchange_info(Some(sym))
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?,
        };
        from_info = match seg {
            "future" | "futures" | "usdt_futures" | "fapi" => {
                futures_commission_hint_from_exchange_info(&raw, sym)
            }
            _ => spot_commission_hint_from_exchange_info(&raw, sym),
        };
        if from_info.is_some() {
            source = "exchange_info";
        }
    }

    let defaults: CommissionBps = from_info.unwrap_or_else(|| match seg {
        "future" | "futures" | "usdt_futures" | "fapi" => default_usdt_futures_commission_bps(),
        _ => default_spot_commission_bps(),
    });
    Ok(Json(json!({
        "segment": seg,
        "query_symbol": sym_upper,
        "defaults_bps": defaults,
        "source": source
    })))
}

fn commission_account_segment_db(seg: &str) -> Result<&'static str, ApiError> {
    match seg {
        "future" | "futures" | "usdt_futures" | "fapi" => Ok("futures"),
        "spot" => Ok("spot"),
        _ => Err(ApiError::bad_request("segment: spot veya futures")),
    }
}

/// Hesaba özel komisyon — Spot: `sapi/v1/asset/tradeFee`, Futures: `fapi/v1/commissionRate`.
async fn binance_commission_account(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Query(q): Query<CommissionAccountQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("geçersiz token sub"))?;
    let sym = q.symbol.trim().to_uppercase();
    if sym.is_empty() {
        return Err(ApiError::bad_request("symbol gerekli"));
    }
    let seg = q.segment.as_deref().unwrap_or("spot");
    let seg_db = commission_account_segment_db(seg)?;

    let creds = st
        .exchange_accounts
        .binance_for_user(user_id, seg_db)
        .await?
        .ok_or_else(|| {
            ApiError::bad_request(format!(
                "Binance {seg_db} API anahtarı yok — exchange_accounts veya yanlış segment (spot vs futures)"
            ))
        })?;

    let cfg = BinanceClientConfig::mainnet_with_keys(creds.api_key, creds.api_secret);
    let client = BinanceClient::new(cfg).map_err(|e| ApiError::internal(e.to_string()))?;

    let (maker, taker, source) = if seg_db == "futures" {
        let raw = client
            .fapi_commission_rate(&sym)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let (m, t) = commission_rate_from_fapi_response(&raw).ok_or_else(|| {
            ApiError::internal(format!("fapi commissionRate ayrıştırılamadı: {raw}"))
        })?;
        (m, t, "fapi_v1_commissionRate")
    } else {
        let raw = client
            .sapi_asset_trade_fee(Some(&sym))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let (m, t) = trade_fee_from_sapi_response(&raw, &sym).ok_or_else(|| {
            ApiError::internal(format!(
                "sapi asset/tradeFee yanıtında sembol yok veya format: {raw}"
            ))
        })?;
        (m, t, "sapi_v1_asset_tradeFee")
    };

    Ok(Json(json!({
        "symbol": sym,
        "segment": seg_db,
        "maker_rate": maker.to_string(),
        "taker_rate": taker.to_string(),
        "source": source
    })))
}

/// USDT-M kaldıraç — `POST /fapi/v1/leverage` (JWT + futures `exchange_accounts`).
async fn binance_futures_leverage(
    Extension(claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<FuturesLeverageBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::bad_request("geçersiz token sub"))?;
    let sym = body.symbol.trim().to_uppercase();
    if sym.is_empty() {
        return Err(ApiError::bad_request("symbol gerekli"));
    }
    let leverage = body.leverage.clamp(1, 125);

    let creds = st
        .exchange_accounts
        .binance_for_user(user_id, "futures")
        .await?
        .ok_or_else(|| {
            ApiError::bad_request(
                "Binance futures API anahtarı yok — exchange_accounts (segment futures)",
            )
        })?;

    let cfg = BinanceClientConfig::mainnet_with_keys(creds.api_key, creds.api_secret);
    let client = BinanceClient::new(cfg).map_err(|e| ApiError::internal(e.to_string()))?;
    let raw = client
        .fapi_change_leverage(&sym, leverage)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(raw))
}

async fn backfill_market_bars_from_rest(
    Extension(_claims): Extension<AccessClaims>,
    State(st): State<SharedState>,
    Json(body): Json<BackfillBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let sym = body.symbol.trim().to_uppercase();
    if sym.is_empty() {
        return Err(ApiError::bad_request("symbol gerekli"));
    }
    let interval = body.interval.trim().to_string();
    if interval.is_empty() {
        return Err(ApiError::bad_request("interval gerekli"));
    }
    let seg = body.segment.as_deref().unwrap_or("spot");
    let seg_db = match seg {
        "future" | "futures" | "usdt_futures" | "fapi" => "futures",
        _ => "spot",
    };
    let target = i64::from(body.limit.unwrap_or(500).clamp(1, 50_000));
    let upserted = backfill_binance_public_klines(&st.pool, &sym, &interval, seg, target)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

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
) -> Result<Json<Vec<qtss_storage::MarketBarRow>>, ApiError> {
    let limit = q.limit.unwrap_or(500).clamp(1, 50_000);
    let bars = list_recent_bars(
        &st.pool,
        q.exchange.trim(),
        q.segment.trim(),
        q.symbol.trim(),
        q.interval.trim(),
        limit,
    )
    .await?;
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
