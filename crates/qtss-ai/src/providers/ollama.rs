//! Ollama `/api/chat` (on-prem).

use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};

use super::{AiCompletionProvider, AiRequest, AiResponse};
use crate::error::{AiError, AiResult};

#[derive(Debug, Clone)]
pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
}

impl OllamaProvider {
    pub fn from_settings(base_url: String, timeout_secs: u64) -> AiResult<Self> {
        if base_url.trim().is_empty() {
            return Err(AiError::ProviderNotConfigured("QTSS_AI_OLLAMA_BASE_URL".into()));
        }
        let base_url = base_url.trim_end_matches('/').to_string();
        let timeout_secs = timeout_secs.max(30);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent(concat!("qtss-ai/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| AiError::http(format!("reqwest: {e}")))?;
        Ok(Self { client, base_url })
    }

    pub fn from_env() -> AiResult<Self> {
        let base_url = std::env::var("QTSS_AI_OLLAMA_BASE_URL")
            .or_else(|_| std::env::var("OLLAMA_HOST"))
            .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
        let timeout_secs = std::env::var("QTSS_AI_ONPREM_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(180_u64);
        Self::from_settings(base_url, timeout_secs)
    }

    fn url_chat(&self) -> String {
        format!("{}/api/chat", self.base_url)
    }
}

#[derive(Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
}

#[derive(Serialize)]
struct OllamaReq<'a> {
    model: &'a str,
    messages: Vec<OllamaMsg<'a>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Serialize)]
struct OllamaMsg<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OllamaResp {
    message: Option<OllamaMsgOut>,
}

#[derive(Deserialize)]
struct OllamaMsgOut {
    content: Option<String>,
}

#[async_trait]
impl AiCompletionProvider for OllamaProvider {
    fn provider_id(&self) -> &'static str {
        "ollama"
    }

    async fn complete(&self, req: &AiRequest) -> AiResult<AiResponse> {
        let mut messages = Vec::new();
        if let Some(ref sys) = req.system {
            if !sys.is_empty() {
                messages.push(OllamaMsg {
                    role: "system",
                    content: sys.as_str(),
                });
            }
        }
        messages.push(OllamaMsg {
            role: "user",
            content: &req.user,
        });
        let body = OllamaReq {
            model: req.model.as_str(),
            messages,
            stream: false,
            options: Some(OllamaOptions {
                temperature: Some(req.temperature),
                num_predict: Some(req.max_tokens),
            }),
        };
        let timeout = std::time::Duration::from_secs(req.suggested_timeout_secs());
        let res = self
            .client
            .post(self.url_chat())
            .timeout(timeout)
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AiError::http(e.to_string()))?;
        let status = res.status();
        let txt = res.text().await.map_err(|e| AiError::http(e.to_string()))?;
        if !status.is_success() {
            let preview: String = txt.chars().take(500).collect();
            return Err(AiError::http(format!("ollama HTTP {}: {}", status, preview)));
        }
        let parsed: OllamaResp = serde_json::from_str(&txt).map_err(|e| {
            let preview: String = txt.chars().take(500).collect();
            AiError::http(format!("ollama json: {e}; body: {preview}"))
        })?;
        let text = parsed
            .message
            .and_then(|m| m.content)
            .unwrap_or_default();
        if text.is_empty() {
            return Err(AiError::http("ollama empty message.content"));
        }
        Ok(AiResponse {
            text,
            model: req.model.clone(),
            provider_id: self.provider_id().to_string(),
            usage: None,
        })
    }
}
