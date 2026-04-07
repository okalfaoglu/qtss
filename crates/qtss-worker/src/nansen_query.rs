//! Ortak token screener istek gövdesi ve API tabanı (`nansen_engine` + `setup_scan_engine`).
//!
//! Varsayılan gövde, Nansen OpenAPI `TokenScreenerRequest` ile uyumludur: `trader_type: "sm"`,
//! akıllı para + hacim/netflow odaklı sıralama, erken aşama yaş aralığı, stabilcoin hariç.
//!
//! **Sıra (`token_screener_body`):** `app_config` (`QTSS_NANSEN_CONFIG_KEY`, varsayılan
//! `nansen_screener_request`) → `NANSEN_TOKEN_SCREENER_REQUEST_JSON` → [`default_token_screener_body`].
//! Admin: `PUT /api/v1/config` ile `key` / `value` JSON; worker restart gerekmez.

use qtss_storage::AppConfigRepository;
use serde_json::json;
use sqlx::PgPool;
use tracing::{debug, warn};

pub fn default_token_screener_body() -> serde_json::Value {
    json!({
        "chains": ["ethereum", "solana", "base", "arbitrum", "bnb"],
        "timeframe": "24h",
        "filters": {
            "trader_type": "all",
            "token_age_days": { "min": 1 },
            "liquidity": { "min": 5000 },
            "volume": { "min": 1000 },
            "include_stablecoins": false
        },
        "order_by": [
            { "field": "volume", "direction": "DESC" },
            { "field": "netflow", "direction": "DESC" }
        ],
        "pagination": { "page": 1, "per_page": 500 }
    })
}

pub fn token_screener_body_from_env() -> serde_json::Value {
    match std::env::var("NANSEN_TOKEN_SCREENER_REQUEST_JSON") {
        Ok(raw) if !raw.trim().is_empty() => {
            match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(v) => v,
                Err(e) => {
                    warn!(%e, "NANSEN_TOKEN_SCREENER_REQUEST_JSON geçersiz JSON — varsayılan gövde");
                    default_token_screener_body()
                }
            }
        }
        _ => default_token_screener_body(),
    }
}

/// `app_config` anahtarı; preset değiştirmek için `nansen_screener_request_v2` gibi ayrı satır + bu env.
pub fn nansen_app_config_key() -> String {
    std::env::var("QTSS_NANSEN_CONFIG_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "nansen_screener_request".to_string())
}

fn is_usable_screener_body(v: &serde_json::Value) -> bool {
    matches!(
        v.get("chains").and_then(|c| c.as_array()),
        Some(a) if !a.is_empty()
    )
}

/// Her döngüde çağrılır: DB’de geçerli gövde varsa anında yeni filtreler uygulanır.
pub async fn token_screener_body(pool: &PgPool) -> serde_json::Value {
    let key = nansen_app_config_key();
    match AppConfigRepository::get_value_json(pool, key.trim()).await {
        Ok(Some(v)) if is_usable_screener_body(&v) => {
            debug!(config_key = %key, "nansen token screener gövdesi app_config’ten");
            v
        }
        Ok(Some(v)) => {
            warn!(
                config_key = %key,
                value = %v,
                "app_config’teki Nansen screener JSON kullanılamıyor (chains boş veya yok) — env/varsayılan"
            );
            token_screener_body_from_env()
        }
        Ok(None) => token_screener_body_from_env(),
        Err(e) => {
            warn!(%e, config_key = %key, "app_config okunamadı — env/varsayılan");
            token_screener_body_from_env()
        }
    }
}

pub fn nansen_api_base() -> String {
    std::env::var("NANSEN_API_BASE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| qtss_nansen::default_api_base().to_string())
}
