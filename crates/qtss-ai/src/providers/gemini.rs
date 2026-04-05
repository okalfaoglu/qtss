//! Google Gemini `generateContent` (v1beta) — text completions for qtss-ai.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{AiCompletionProvider, AiRequest, AiResponse};
use crate::error::{AiError, AiResult};

/// Default Google AI Studio REST base (`models/{id}:generateContent`).
pub const DEFAULT_GEMINI_API_ROOT: &str = "https://generativelanguage.googleapis.com/v1beta";

#[derive(Debug, Clone)]
pub struct GeminiProvider {
    client: reqwest::Client,
    api_key: String,
    api_root: String,
    timeout_floor_secs: u64,
}

impl GeminiProvider {
    pub fn from_settings(api_key: String, api_root: String, timeout_floor_secs: u64) -> AiResult<Self> {
        if api_key.trim().is_empty() {
            return Err(AiError::ProviderNotConfigured("Gemini API key empty".into()));
        }
        let api_root = if api_root.trim().is_empty() {
            DEFAULT_GEMINI_API_ROOT.to_string()
        } else {
            api_root.trim_end_matches('/').to_string()
        };
        let timeout_floor_secs = timeout_floor_secs.max(30);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_floor_secs.max(120)))
            .user_agent(concat!("qtss-ai/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| AiError::http(format!("reqwest: {e}")))?;
        Ok(Self {
            client,
            api_key: api_key.trim().to_string(),
            api_root,
            timeout_floor_secs,
        })
    }

    fn url_generate(&self, model: &str) -> String {
        let m = urlencoding::encode(model.trim());
        let k = urlencoding::encode(self.api_key.trim());
        format!("{}/models/{}:generateContent?key={}", self.api_root, m, k)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<SystemInstruction<'a>>,
    contents: Vec<Content<'a>>,
    generation_config: GenerationConfig,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemInstruction<'a> {
    parts: Vec<PartText<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Content<'a> {
    role: &'static str,
    parts: Vec<PartText<'a>>,
}

#[derive(Serialize)]
struct PartText<'a> {
    text: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    max_output_tokens: u32,
    temperature: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMeta {
    prompt_token_count: Option<u64>,
    candidates_token_count: Option<u64>,
}

#[async_trait]
impl AiCompletionProvider for GeminiProvider {
    fn provider_id(&self) -> &'static str {
        "gemini"
    }

    async fn complete(&self, req: &AiRequest) -> AiResult<AiResponse> {
        let system_instruction = req.system.as_ref().map(|s| SystemInstruction {
            parts: vec![PartText { text: s.as_str() }],
        });
        let body = GenerateBody {
            system_instruction,
            contents: vec![Content {
                role: "user",
                parts: vec![PartText {
                    text: req.user.as_str(),
                }],
            }],
            generation_config: GenerationConfig {
                max_output_tokens: req.max_tokens.max(1),
                temperature: req.temperature,
            },
        };
        let url = self.url_generate(req.model.as_str());
        let per_req_timeout = req
            .suggested_timeout_secs()
            .max(self.timeout_floor_secs);
        let res = self
            .client
            .post(&url)
            .timeout(Duration::from_secs(per_req_timeout))
            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .json(&body)
            .send()
            .await
            .map_err(|e| AiError::http(e.to_string()))?;
        let status = res.status();
        let txt = res.text().await.map_err(|e| AiError::http(e.to_string()))?;
        if !status.is_success() {
            let preview = truncate_chars(&txt, 500);
            return Err(AiError::http(format!("gemini HTTP {}: {}", status, preview)));
        }
        let v: Value = serde_json::from_str(&txt).map_err(|e| {
            let preview = truncate_chars(&txt, 500);
            AiError::http(format!("gemini json: {e}; body: {preview}"))
        })?;
        if let Some(err) = v.get("error") {
            let preview = err.to_string();
            let preview = truncate_chars(&preview, 500);
            return Err(AiError::http(format!("gemini error: {preview}")));
        }
        let parts_arr = v["candidates"]
            .get(0)
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
            .ok_or_else(|| {
                AiError::http(format!(
                    "gemini missing candidates[0].content.parts; keys: {:?}",
                    v.as_object().map(|m| m.keys().collect::<Vec<_>>())
                ))
            })?;
        let mut out = String::new();
        for p in parts_arr {
            if let Some(t) = p.get("text").and_then(|x| x.as_str()) {
                out.push_str(t);
            }
        }
        if out.trim().is_empty() {
            return Err(AiError::http(
                "gemini empty text in candidates[0].content.parts".to_string(),
            ));
        }
        let usage = v
            .get("usageMetadata")
            .and_then(|u| serde_json::from_value::<GeminiUsageMeta>(u.clone()).ok())
            .map(|u| super::AiUsage {
                input_tokens: u.prompt_token_count,
                output_tokens: u.candidates_token_count,
            });
        Ok(AiResponse {
            text: out,
            model: req.model.clone(),
            provider_id: self.provider_id().to_string(),
            usage,
        })
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}
