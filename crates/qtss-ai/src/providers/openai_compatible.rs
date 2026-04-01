//! OpenAI-compatible HTTP (`/v1/chat/completions`) for vLLM, TGI, LM Studio, internal gateways.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use super::{AiCompletionProvider, AiRequest, AiResponse};
use crate::error::{AiError, AiResult};

static ONPREM_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    extra_headers: Vec<(String, String)>,
    max_in_flight: usize,
    provider_id: &'static str,
}

impl OpenAiCompatibleProvider {
    pub fn from_settings(
        base_url: String,
        api_key: Option<String>,
        extra_headers_json: Option<String>,
        timeout_secs: u64,
        max_in_flight: usize,
    ) -> AiResult<Self> {
        if base_url.trim().is_empty() {
            return Err(AiError::ProviderNotConfigured(
                "QTSS_AI_OPENAI_COMPAT_BASE_URL".into(),
            ));
        }
        let base_url = base_url.trim_end_matches('/').to_string();
        let timeout_secs = timeout_secs.max(30);
        let extra_headers = parse_headers_json(extra_headers_json);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent(concat!("qtss-ai/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| AiError::http(format!("reqwest: {e}")))?;
        Ok(Self {
            client,
            base_url,
            api_key,
            extra_headers,
            max_in_flight: max_in_flight.max(1),
            provider_id: "openai_compatible",
        })
    }

    pub fn from_env() -> AiResult<Self> {
        let base_url = std::env::var("QTSS_AI_OPENAI_COMPAT_BASE_URL")
            .or_else(|_| std::env::var("OPENAI_BASE_URL"))
            .unwrap_or_else(|_| String::new());
        let timeout_secs = std::env::var("QTSS_AI_ONPREM_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(180_u64);
        let max_in_flight = std::env::var("QTSS_AI_ONPREM_MAX_IN_FLIGHT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4_usize);
        let api_key = std::env::var("QTSS_AI_ONPREM_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("OPENAI_API_KEY").ok().filter(|s| !s.trim().is_empty()));
        let headers = std::env::var("QTSS_AI_OPENAI_COMPAT_HEADERS_JSON").ok();
        Self::from_settings(base_url, api_key, headers, timeout_secs, max_in_flight)
    }

    fn url_chat(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url)
    }

    fn build_headers(&self) -> AiResult<HeaderMap> {
        let mut h = HeaderMap::new();
        h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(ref k) = self.api_key {
            h.insert(
                "Authorization",
                HeaderValue::from_str(&format!("Bearer {}", k.trim()))
                    .map_err(|e| AiError::http(e.to_string()))?,
            );
        }
        for (name, val) in &self.extra_headers {
            let hn = HeaderName::from_bytes(name.as_bytes()).map_err(|e| AiError::http(e.to_string()))?;
            let hv = HeaderValue::from_str(val).map_err(|e| AiError::http(e.to_string()))?;
            h.insert(hn, hv);
        }
        Ok(h)
    }
}

fn parse_headers_json(raw: Option<String>) -> Vec<(String, String)> {
    let Some(s) = raw else {
        return vec![];
    };
    let Ok(v) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&s) else {
        return vec![];
    };
    v.into_iter()
        .filter_map(|(k, val)| {
            let vs = match val {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            Some((k, vs))
        })
        .collect()
}

#[derive(Serialize)]
struct ChatReq<'a> {
    model: &'a str,
    messages: Vec<ChatMsg<'a>>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Serialize)]
struct ChatMsg<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResp {
    choices: Vec<Choice>,
    model: Option<String>,
}

#[derive(Deserialize)]
struct Choice {
    message: Msg,
}

#[derive(Deserialize)]
struct Msg {
    content: Option<String>,
}

#[async_trait]
impl AiCompletionProvider for OpenAiCompatibleProvider {
    fn provider_id(&self) -> &'static str {
        self.provider_id
    }

    async fn complete(&self, req: &AiRequest) -> AiResult<AiResponse> {
        let prev = ONPREM_IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
        if prev + 1 > self.max_in_flight {
            ONPREM_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
            return Err(AiError::http(format!(
                "on-prem max in-flight exceeded ({})",
                self.max_in_flight
            )));
        }
        let result = self.complete_inner(req).await;
        ONPREM_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
        result
    }
}

impl OpenAiCompatibleProvider {
    async fn complete_inner(&self, req: &AiRequest) -> AiResult<AiResponse> {
        let mut messages = Vec::new();
        if let Some(ref sys) = req.system {
            if !sys.is_empty() {
                messages.push(ChatMsg {
                    role: "system",
                    content: sys.as_str(),
                });
            }
        }
        messages.push(ChatMsg {
            role: "user",
            content: &req.user,
        });
        let body = ChatReq {
            model: req.model.as_str(),
            messages,
            max_tokens: req.max_tokens.max(1),
            temperature: req.temperature,
        };
        let headers = self.build_headers()?;
        let res = self
            .client
            .post(self.url_chat())
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| AiError::http(e.to_string()))?;
        let status = res.status();
        let txt = res.text().await.map_err(|e| AiError::http(e.to_string()))?;
        if !status.is_success() {
            let preview: String = txt.chars().take(500).collect();
            return Err(AiError::http(format!("openai_compat HTTP {}: {}", status, preview)));
        }
        let parsed: ChatResp = serde_json::from_str(&txt).map_err(|e| {
            let preview: String = txt.chars().take(500).collect();
            AiError::http(format!("openai_compat json: {e}; body: {preview}"))
        })?;
        let text = parsed
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();
        if text.is_empty() {
            return Err(AiError::http(
                "openai_compat empty choices[0].message.content",
            ));
        }
        Ok(AiResponse {
            text,
            model: parsed.model.unwrap_or_else(|| req.model.clone()),
            provider_id: self.provider_id().to_string(),
        })
    }
}
