//! Ortak token screener istek gövdesi ve API tabanı (`nansen_engine` + `setup_scan_engine`).
//!
//! Varsayılan gövde, Nansen OpenAPI `TokenScreenerRequest` ile uyumludur: `trader_type: "sm"`,
//! akıllı para + hacim/netflow odaklı sıralama, erken aşama yaş aralığı, stabilcoin hariç.
//! Özelleştirmek için `NANSEN_TOKEN_SCREENER_REQUEST_JSON` kullanın.

use serde_json::json;
use tracing::warn;

pub fn default_token_screener_body() -> serde_json::Value {
    json!({
        "chains": ["ethereum", "solana", "base", "arbitrum", "optimism"],
        "timeframe": "6h",
        "filters": {
            "trader_type": "sm",
            "token_age_days": { "min": 1, "max": 150 },
            "liquidity": { "min": 8000 },
            "volume": { "min": 5000 },
            "price_change": { "min": -12, "max": 22 },
            "nof_traders": { "min": 2, "max": 75 },
            "include_stablecoins": false
        },
        "order_by": [
            { "field": "volume", "direction": "DESC" },
            { "field": "netflow", "direction": "DESC" }
        ],
        "pagination": { "page": 1, "per_page": 250 }
    })
}

pub fn token_screener_body_from_env() -> serde_json::Value {
    match std::env::var("NANSEN_TOKEN_SCREENER_REQUEST_JSON") {
        Ok(raw) if !raw.trim().is_empty() => match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(v) => v,
            Err(e) => {
                warn!(%e, "NANSEN_TOKEN_SCREENER_REQUEST_JSON geçersiz JSON — varsayılan gövde");
                default_token_screener_body()
            }
        },
        _ => default_token_screener_body(),
    }
}

pub fn nansen_api_base() -> String {
    std::env::var("NANSEN_API_BASE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| qtss_nansen::default_api_base().to_string())
}
