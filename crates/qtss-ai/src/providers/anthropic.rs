//! Anthropic Messages API (cloud reference implementation).

use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{AiCompletionProvider, AiRequest, AiResponse};
use crate::error::{AiError, AiResult};

const DEFAULT_BASE: &str = "https://api.anthropic.com";
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl AnthropicProvider {
    pub fn from_settings(api_key: String, base_url: String, timeout_secs: u64) -> AiResult<Self> {
        if api_key.trim().is_empty() {
            return Err(AiError::ProviderNotConfigured("ANTHROPIC_API_KEY empty".into()));
        }
        let base_url = base_url.trim_end_matches('/').to_string();
        let timeout_secs = timeout_secs.max(30);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent(concat!("qtss-ai/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| AiError::http(format!("reqwest: {e}")))?;
        Ok(Self {
            client,
            api_key,
            base_url,
        })
    }

    pub fn from_env() -> AiResult<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("QTSS_AI_ANTHROPIC_API_KEY"))
            .map_err(|_| AiError::ProviderNotConfigured("ANTHROPIC_API_KEY".into()))?;
        let base_url = std::env::var("ANTHROPIC_BASE_URL")
            .or_else(|_| std::env::var("QTSS_AI_ANTHROPIC_BASE_URL"))
            .unwrap_or_else(|_| DEFAULT_BASE.to_string());
        let timeout_secs = std::env::var("QTSS_AI_ANTHROPIC_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        Self::from_settings(api_key, base_url, timeout_secs)
    }

    fn url_messages(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }
}

#[derive(Serialize)]
struct MessagesBody<'a> {
    model: &'a str,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    messages: Vec<Message<'a>>,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct MessagesResp {
    content: Vec<ContentBlock>,
    model: Option<String>,
    usage: Option<AnthropicUsage>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

#[async_trait]
impl AiCompletionProvider for AnthropicProvider {
    fn provider_id(&self) -> &'static str {
        "anthropic"
    }

    async fn complete(&self, req: &AiRequest) -> AiResult<AiResponse> {
        let body = MessagesBody {
            model: req.model.as_str(),
            max_tokens: req.max_tokens.max(1),
            temperature: Some(req.temperature),
            system: req.system.as_deref(),
            messages: vec![Message {
                role: "user",
                content: &req.user,
            }],
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&self.api_key).map_err(|e| AiError::http(e.to_string()))?,
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let res = self
            .client
            .post(self.url_messages())
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| AiError::http(e.to_string()))?;
        let status = res.status();
        let txt = res.text().await.map_err(|e| AiError::http(e.to_string()))?;
        if !status.is_success() {
            let preview = truncate_chars(&txt, 500);
            return Err(AiError::http(format!("anthropic HTTP {}: {}", status, preview)));
        }
        let parsed: MessagesResp = serde_json::from_str(&txt).map_err(|e| {
            let preview = truncate_chars(&txt, 500);
            AiError::http(format!("anthropic json: {e}; body: {preview}"))
        })?;
        let mut out = String::new();
        for b in parsed.content {
            if b.block_type == "text" {
                if let Some(t) = b.text {
                    out.push_str(&t);
                }
            }
        }
        if out.is_empty() {
            // try raw fallback for debugging
            let v: Value = serde_json::from_str(&txt).unwrap_or(Value::Null);
            return Err(AiError::http(format!(
                "anthropic empty content; keys: {:?}",
                v.as_object().map(|m| m.keys().collect::<Vec<_>>())
            )));
        }
        let usage = parsed.usage.map(|u| super::AiUsage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
        });
        Ok(AiResponse {
            text: out,
            model: parsed.model.unwrap_or_else(|| req.model.clone()),
            provider_id: self.provider_id().to_string(),
            usage,
        })
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}
