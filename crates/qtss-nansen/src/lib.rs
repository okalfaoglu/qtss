//! HTTP client for [Nansen API](https://docs.nansen.ai/) (`api.nansen.ai` POST + optional [`default_app_base`] GET for [Points](https://docs.nansen.ai/api/points.md)).
//! API keys are supplied by the caller for `api.nansen.ai` routes; this crate does not read env.

use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

const DEFAULT_BASE: &str = "https://api.nansen.ai";
/// Base URL for [Permissionless Points](https://docs.nansen.ai/api/points.md) (`GET`, no API key).
const DEFAULT_APP_BASE: &str = "https://app.nansen.ai";

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

fn nansen_meta_from_headers(headers: &HeaderMap) -> NansenResponseMeta {
    NansenResponseMeta {
        credits_used: header_first(headers, "x-nansen-credits-used")
            .or_else(|| header_first(headers, "X-Nansen-Credits-Used")),
        credits_remaining: header_first(headers, "x-nansen-credits-remaining")
            .or_else(|| header_first(headers, "X-Nansen-Credits-Remaining")),
        rate_limit_remaining: header_first(headers, "ratelimit-remaining")
            .or_else(|| header_first(headers, "RateLimit-Remaining")),
    }
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
    let meta = nansen_meta_from_headers(&headers);

    let text = res.text().await?;
    if !(200..300).contains(&status) {
        return Err(NansenError::Api {
            status,
            body: text.chars().take(4000).collect(),
        });
    }

    // Log credit consumption for every successful Nansen API call
    if let Some(ref used) = meta.credits_used {
        let remaining = meta.credits_remaining.as_deref().unwrap_or("?");
        tracing::info!(
            credits_used = %used,
            credits_remaining = %remaining,
            endpoint = %path,
            "nansen API credit consumption"
        );
    }

    let v: Value = serde_json::from_str(&text)?;
    Ok((v, meta))
}

async fn get_json_url(
    client: &reqwest::Client,
    url: &str,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    let res = client.get(url).send().await?;
    let status = res.status().as_u16();
    let headers = res.headers().clone();
    let meta = nansen_meta_from_headers(&headers);
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

/// POST with JSON body; response body is returned as text (no JSON parse). For `text/event-stream` (Agent) or other non-JSON 2xx bodies.
async fn post_text_path(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    path: &str,
    body: &Value,
) -> Result<(String, NansenResponseMeta), NansenError> {
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
    let meta = nansen_meta_from_headers(res.headers());

    let text = res.text().await?;
    if !(200..300).contains(&status) {
        return Err(NansenError::Api {
            status,
            body: text.chars().take(4000).collect(),
        });
    }
    Ok((text, meta))
}

/// Generic `POST` with JSON body and JSON response. `relative_path` is e.g. `api/v1/portfolio/defi-holdings` (leading slash optional).
pub async fn post_json_relative(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    relative_path: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    let path = relative_path.trim().trim_start_matches('/');
    post_json_path(client, base_url, api_key, path, body).await
}

/// `POST /api/v1/search/general` — tokens and entities by name, symbol, or address ([docs](https://docs.nansen.ai/api/search.md)).
pub async fn post_search_general(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(client, base_url, api_key, "api/v1/search/general", body).await
}

/// `POST /api/v1/search/entity-name` — resolve `entity_name` strings for profiler-style requests (0 credits; [docs](https://docs.nansen.ai/api/profiler/entity-name-search.md)).
pub async fn post_search_entity_name(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(client, base_url, api_key, "api/v1/search/entity-name", body).await
}

/// `POST /api/v1/token-screener` — request body is API-specific JSON (chains, filters, pagination).
pub async fn post_token_screener(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(client, base_url, api_key, "api/v1/token-screener", body).await
}

/// `POST` smart-money netflow — OpenAPI path is [`/api/v1/smart-money/netflow`](https://docs.nansen.ai/api/smart-money/netflows.md).
/// Pass `relative_path` (e.g. `api/v1/smart-money/netflow`) from `NANSEN_SMART_MONEY_NETFLOW_PATH` if your plan uses an alias.
pub async fn post_smart_money_netflow(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    relative_path: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    let path = relative_path.trim().trim_start_matches('/');
    post_json_path(client, base_url, api_key, path, body).await
}

/// Same as [`post_smart_money_netflow`] with canonical path `api/v1/smart-money/netflow`.
pub async fn post_smart_money_netflows(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_smart_money_netflow(client, base_url, api_key, "api/v1/smart-money/netflow", body).await
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

/// `POST /api/v1/smart-money/dex-trades` — last-24h DEX trades from smart-money wallets.
pub async fn post_smart_money_dex_trades(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/smart-money/dex-trades",
        body,
    )
    .await
}

/// `POST /api/v1/tgm/flows` — holder-category flow time series (`chain`, `token_address`, `date` required).
pub async fn post_tgm_flows(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(client, base_url, api_key, "api/v1/tgm/flows", body).await
}

/// `POST /api/v1/tgm/perp-trades` — Hyperliquid perp trades for a `token_symbol` + `date` range.
pub async fn post_tgm_perp_trades(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(client, base_url, api_key, "api/v1/tgm/perp-trades", body).await
}

/// `POST /api/v1/tgm/dex-trades` — DEX trades for a token.
pub async fn post_tgm_dex_trades(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(client, base_url, api_key, "api/v1/tgm/dex-trades", body).await
}

/// `POST /api/v1/tgm/token-information`
pub async fn post_tgm_token_information(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/tgm/token-information",
        body,
    )
    .await
}

/// `POST /api/v1/tgm/indicators` — Nansen risk/reward indicators (`chain`, `token_address`).
pub async fn post_tgm_indicators(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(client, base_url, api_key, "api/v1/tgm/indicators", body).await
}

/// `POST /api/v1/tgm/perp-positions` — open perp positions for a perp token.
pub async fn post_tgm_perp_positions(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/tgm/perp-positions",
        body,
    )
    .await
}

/// `POST /api/v1/tgm/holders`
pub async fn post_tgm_holders(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(client, base_url, api_key, "api/v1/tgm/holders", body).await
}

/// `POST /api/v1/perp-screener` — Hyperliquid perp screener (OpenAPI path; not under `tgm/`).
pub async fn post_perp_screener(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(client, base_url, api_key, "api/v1/perp-screener", body).await
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

/// `POST /api/v1/smart-money/historical-holdings`
pub async fn post_smart_money_historical_holdings(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/smart-money/historical-holdings",
        body,
    )
    .await
}

/// `POST /api/v1/smart-money/dcas` — Jupiter DCAs from smart-money wallets.
pub async fn post_smart_money_dcas(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(client, base_url, api_key, "api/v1/smart-money/dcas", body).await
}

/// `POST /api/v1/tgm/transfers`
pub async fn post_tgm_transfers(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(client, base_url, api_key, "api/v1/tgm/transfers", body).await
}

/// `POST /api/v1/tgm/pnl-leaderboard`
pub async fn post_tgm_pnl_leaderboard(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/tgm/pnl-leaderboard",
        body,
    )
    .await
}

/// `POST /api/v1/tgm/token-ohlcv`
pub async fn post_tgm_token_ohlcv(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/tgm/token-ohlcv",
        body,
    )
    .await
}

/// `POST /api/v1/tgm/jup-dca`
pub async fn post_tgm_jup_dca(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(client, base_url, api_key, "api/v1/tgm/jup-dca", body).await
}

/// `POST /api/v1/portfolio/defi-holdings`
pub async fn post_portfolio_defi_holdings(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/portfolio/defi-holdings",
        body,
    )
    .await
}

/// `POST /api/v1/perp-leaderboard` — Hyperliquid address leaderboard (distinct from TGM perp PnL leaderboard).
pub async fn post_perp_leaderboard(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(client, base_url, api_key, "api/v1/perp-leaderboard", body).await
}

/// `POST /api/v1/profiler/address/current-balance`
pub async fn post_profiler_address_current_balance(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/profiler/address/current-balance",
        body,
    )
    .await
}

/// `POST /api/v1/profiler/address/historical-balances`
pub async fn post_profiler_address_historical_balances(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/profiler/address/historical-balances",
        body,
    )
    .await
}

/// `POST /api/v1/profiler/address/transactions`
pub async fn post_profiler_address_transactions(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/profiler/address/transactions",
        body,
    )
    .await
}

/// `POST /api/v1/profiler/perp-trades` — wallet Hyperliquid perp trades (not [`post_smart_money_perp_trades`]).
pub async fn post_profiler_perp_trades(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/profiler/perp-trades",
        body,
    )
    .await
}

/// `POST /api/v1/profiler/address/related-wallets`
pub async fn post_profiler_address_related_wallets(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/profiler/address/related-wallets",
        body,
    )
    .await
}

/// `POST /api/v1/profiler/address/pnl-summary`
pub async fn post_profiler_address_pnl_summary(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/profiler/address/pnl-summary",
        body,
    )
    .await
}

/// `POST /api/v1/profiler/address/pnl`
pub async fn post_profiler_address_pnl(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/profiler/address/pnl",
        body,
    )
    .await
}

/// `POST /api/v1/profiler/address/counterparties`
pub async fn post_profiler_address_counterparties(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/profiler/address/counterparties",
        body,
    )
    .await
}

/// `POST /api/v1/profiler/address/labels` — non-premium labels only.
pub async fn post_profiler_address_labels(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/profiler/address/labels",
        body,
    )
    .await
}

/// `POST /api/v1/profiler/address/premium-labels`
pub async fn post_profiler_address_premium_labels(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/profiler/address/premium-labels",
        body,
    )
    .await
}

/// `POST /api/v1/transaction-with-token-transfer-lookup`
pub async fn post_transaction_with_token_transfer_lookup(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    post_json_path(
        client,
        base_url,
        api_key,
        "api/v1/transaction-with-token-transfer-lookup",
        body,
    )
    .await
}

/// `POST /api/v1/agent/fast` — response is `text/event-stream` (SSE); full stream is buffered into a `String`. Do not parse as JSON.
pub async fn post_agent_fast_raw(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(String, NansenResponseMeta), NansenError> {
    post_text_path(client, base_url, api_key, "api/v1/agent/fast", body).await
}

/// `POST /api/v1/agent/expert` — same streaming semantics as [`post_agent_fast_raw`].
pub async fn post_agent_expert_raw(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(String, NansenResponseMeta), NansenError> {
    post_text_path(client, base_url, api_key, "api/v1/agent/expert", body).await
}

/// Default host for [`get_points_leaderboard`] / [`get_points_wallet_tier`] ([Points API](https://docs.nansen.ai/api/points.md); no API key).
#[must_use]
pub fn default_app_base() -> &'static str {
    DEFAULT_APP_BASE
}

/// `GET /api/points-leaderboard/api` — paginated permissionless points leaderboard (`tier`, `page`, `recordsPerPage` query params).
pub async fn get_points_leaderboard(
    client: &reqwest::Client,
    app_base_url: &str,
    tier: Option<&str>,
    page: Option<u32>,
    records_per_page: Option<u32>,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    let base = app_base_url.trim_end_matches('/');
    let mut url = format!("{base}/api/points-leaderboard/api");
    let mut sep = '?';
    if let Some(t) = tier {
        url.push_str(&format!("{sep}tier={}", urlencoding::encode(t)));
        sep = '&';
    }
    if let Some(p) = page {
        url.push_str(&format!("{sep}page={p}"));
        sep = '&';
    }
    if let Some(n) = records_per_page {
        url.push_str(&format!("{sep}recordsPerPage={n}"));
    }
    get_json_url(client, &url).await
}

/// `GET /api/points-leaderboard/{address}` — tier lookup for one EVM or Solana address (no API key).
pub async fn get_points_wallet_tier(
    client: &reqwest::Client,
    app_base_url: &str,
    address: &str,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    let base = app_base_url.trim_end_matches('/');
    let enc = urlencoding::encode(address.trim());
    let url = format!("{base}/api/points-leaderboard/{enc}");
    get_json_url(client, &url).await
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
