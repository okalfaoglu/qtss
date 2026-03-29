//! `DataSourceProvider` for Nansen token screener — PLAN §3 / diyagram `NansenProvider`.
//! Ham çekim + meta `DataSourceFetchOk` içinde; `nansen_persist` çift yazar (`nansen_snapshots` + `data_snapshots`).

use std::time::Instant;

use async_trait::async_trait;
use qtss_nansen::post_token_screener;
use reqwest::Client;
use serde_json::{json, Value};
use sqlx::PgPool;

use super::provider::{DataSourceFetchOk, DataSourceProvider};
use super::registry::NANSEN_TOKEN_SCREENER_DATA_KEY;

use crate::nansen_query::{nansen_api_base, token_screener_body};

#[derive(Clone)]
pub struct NansenTokenScreenerProvider {
    client: Client,
    pool: PgPool,
}

impl NansenTokenScreenerProvider {
    pub fn new(client: Client, pool: PgPool) -> Self {
        Self { client, pool }
    }
}

fn response_token_count_hint(v: &Value) -> Option<usize> {
    v.get("data")
        .and_then(|d| d.as_array())
        .map(|a| a.len())
        .or_else(|| v.get("tokens").and_then(|t| t.as_array()).map(|a| a.len()))
        .or_else(|| v.get("results").and_then(|r| r.as_array()).map(|a| a.len()))
}

#[async_trait]
impl DataSourceProvider for NansenTokenScreenerProvider {
    fn source_key(&self) -> &str {
        NANSEN_TOKEN_SCREENER_DATA_KEY
    }

    async fn fetch(&self) -> DataSourceFetchOk {
        let Some(api_key) = std::env::var("NANSEN_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
        else {
            return DataSourceFetchOk {
                request_json: json!({ "reason": "NANSEN_API_KEY_missing" }),
                response_json: None,
                meta_json: None,
                error: Some("NANSEN_API_KEY not set".into()),
                fetch_duration_ms: None,
            };
        };

        let base = nansen_api_base();
        let body = token_screener_body(&self.pool).await;

        let t0 = Instant::now();
        let ms = || Some(t0.elapsed().as_millis().min(9_999_999_999) as u64);

        match post_token_screener(&self.client, &base, &api_key, &body).await {
            Ok((json, meta)) => {
                let meta_json = json!({
                    "credits_used": meta.credits_used,
                    "credits_remaining": meta.credits_remaining,
                    "rate_limit_remaining": meta.rate_limit_remaining,
                    "response_token_count_hint": response_token_count_hint(&json),
                });
                DataSourceFetchOk {
                    request_json: body,
                    response_json: Some(json),
                    meta_json: Some(meta_json),
                    error: None,
                    fetch_duration_ms: ms(),
                }
            }
            Err(e) => {
                let mut meta = json!({});
                if e.is_insufficient_credits() {
                    meta["nansen_insufficient_credits"] = json!(true);
                }
                DataSourceFetchOk {
                    request_json: body,
                    response_json: None,
                    meta_json: Some(meta),
                    error: Some(e.to_string()),
                    fetch_duration_ms: ms(),
                }
            }
        }
    }
}
