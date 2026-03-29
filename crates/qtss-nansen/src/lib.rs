//! HTTP client for [Nansen API](https://docs.nansen.ai/) — token screener and related calls.
//! API keys must be supplied via environment (`NANSEN_API_KEY`) by the caller; this crate does not read env.

use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

const DEFAULT_BASE: &str = "https://api.nansen.ai";

#[derive(Debug, thiserror::Error)]
pub enum NansenError {
    #[error("HTTP transport: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Nansen API {status}: {body}")]
    Api { status: u16, body: String },
    #[error("invalid JSON body: {0}")]
    Json(#[from] serde_json::Error),
}

impl NansenError {
    /// HTTP 403 ve gövdede Nansen’in “Insufficient credits” yanıtı (hesap kredisi bitmiş).
    #[must_use]
    pub fn is_insufficient_credits(&self) -> bool {
        match self {
            NansenError::Api { status, body } => {
                if *status != 403 {
                    return false;
                }
                let b = body.to_ascii_lowercase();
                b.contains("insufficient credits") || b.contains("insufficient_credits")
            }
            _ => false,
        }
    }

    /// REST hata kodu (`Api` varyantı); ulaşım/JSON hatalarında `None`.
    #[must_use]
    pub fn http_status(&self) -> Option<u16> {
        match self {
            NansenError::Api { status, .. } if *status > 0 => Some(*status),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NansenResponseMeta {
    pub credits_used: Option<String>,
    pub credits_remaining: Option<String>,
    pub rate_limit_remaining: Option<String>,
}

fn header_first(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

async fn post_json_path(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    path: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    let base = base_url.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    let url = format!("{base}/{path}");
    let res = client
        .post(url)
        .header(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )
        .header(
            "apiKey",
            HeaderValue::from_str(api_key.trim()).map_err(|e| NansenError::Api {
                status: 0,
                body: format!("invalid apiKey header: {e}"),
            })?,
        )
        .json(body)
        .send()
        .await?;

    let status = res.status().as_u16();
    let headers = res.headers().clone();
    let meta = NansenResponseMeta {
        credits_used: header_first(&headers, "x-nansen-credits-used")
            .or_else(|| header_first(&headers, "X-Nansen-Credits-Used")),
        credits_remaining: header_first(&headers, "x-nansen-credits-remaining")
            .or_else(|| header_first(&headers, "X-Nansen-Credits-Remaining")),
        rate_limit_remaining: header_first(&headers, "ratelimit-remaining")
            .or_else(|| header_first(&headers, "RateLimit-Remaining")),
    };

    let text = res.text().await?;
    if !(200..300).contains(&status) {
        return Err(NansenError::Api {
            status,
            body: text.chars().take(4000).collect(),
        });
    }

    let v: Value = serde_json::from_str(&text)?;
    Ok((v, meta))
}

/// `POST /api/v1/token-screener` — request body is API-specific JSON (chains, filters, pagination).
pub async fn post_token_screener(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/token-screener",
        body,
    )
    .await
}

/// `POST /api/v1/smart-money/netflow` (çoğu hesapta çoğul `/netflows` 404 döner).
pub async fn post_smart_money_netflows(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/smart-money/netflow",
        body,
    )
    .await
}

/// `POST /api/v1/smart-money/holdings`
pub async fn post_smart_money_holdings(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/smart-money/holdings",
        body,
    )
    .await
}

/// `POST /api/v1/smart-money/perp-trades`
pub async fn post_smart_money_perp_trades(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/smart-money/perp-trades",
        body,
    )
    .await
}

/// `POST /api/v1/tgm/flow-intelligence`
pub async fn post_tgm_flow_intelligence(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/tgm/flow-intelligence",
        body,
    )
    .await
}

/// `POST /api/v1/tgm/who-bought-sold`
pub async fn post_tgm_who_bought_sold(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/tgm/who-bought-sold",
        body,
    )
    .await
}

/// TGM perp PnL leaderboard — varsayılan yol `api/v1/tgm/perp-pnl-leaderboard` (gövde: `token_symbol` + `date`).
/// Eski `profiler/perp-leaderboard` 404; yol `relative_api_path` ile özelleştirilebilir (`NANSEN_PERP_LEADERBOARD_PATH`).
pub async fn post_profiler_perp_leaderboard(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    relative_api_path: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    let path = relative_api_path.trim().trim_start_matches('/');
    post_json_path(client, base_url, api_key, path, body).await
}

/// `POST /api/v1/profiler/perp-positions` — body includes wallet address(es) per Nansen API.
pub async fn post_profiler_perp_positions(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/profiler/perp-positions",
        body,
    )
    .await
}

/// Default production API host when `NANSEN_API_BASE` is unset (caller may read env).
#[must_use]
pub fn default_api_base() -> &'static str {
    DEFAULT_BASE
}

#[cfg(test)]
mod tests {
    use super::NansenError;

    #[test]
    fn insufficient_credits_detected_on_403_body() {
        let e = NansenError::Api {
            status: 403,
            body: r#"{"error":"Insufficient credits","detail":"none"}"#.into(),
        };
        assert!(e.is_insufficient_credits());
    }

    #[test]
    fn insufficient_credits_not_on_401() {
        let e = NansenError::Api {
            status: 401,
            body: r#"{"error":"Insufficient credits"}"#.into(),
        };
        assert!(!e.is_insufficient_credits());
    }
}
