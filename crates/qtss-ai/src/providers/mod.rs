//! Multi-vendor completion providers (cloud + on-prem).

mod anthropic;
mod ollama;
mod openai_compatible;

pub use anthropic::AnthropicProvider;
pub use ollama::OllamaProvider;
pub use openai_compatible::OpenAiCompatibleProvider;

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::AiEngineConfig;
use crate::error::{AiError, AiResult};

/// Layer selector for provider / model resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayerKind {
    Tactical,
    Operational,
    Strategic,
}

impl LayerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tactical => "tactical",
            Self::Operational => "operational",
            Self::Strategic => "strategic",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRequest {
    pub system: Option<String>,
    pub user: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub model: String,
}

impl AiRequest {
    /// Suggested timeout based on `max_tokens` — longer generation needs more time.
    /// ~30 tokens/sec baseline → max_tokens/30 + 30s base.
    pub fn suggested_timeout_secs(&self) -> u64 {
        let gen_secs = (self.max_tokens as u64) / 30;
        (gen_secs + 30).clamp(60, 600)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResponse {
    pub text: String,
    pub model: String,
    pub provider_id: String,
    #[serde(default)]
    pub usage: Option<AiUsage>,
}

#[async_trait]
pub trait AiCompletionProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;

    async fn complete(&self, req: &AiRequest) -> AiResult<AiResponse>;
}

use crate::provider_secrets::AiProviderSecrets;

/// Build the configured provider for a layer (`ai_engine_config.provider_*` + `AiProviderSecrets`).
pub fn provider_for_layer(
    cfg: &AiEngineConfig,
    layer: LayerKind,
    secrets: &AiProviderSecrets,
) -> AiResult<Arc<dyn AiCompletionProvider>> {
    let id = match layer {
        LayerKind::Tactical => cfg.provider_tactical.trim(),
        LayerKind::Operational => cfg.provider_operational.trim(),
        LayerKind::Strategic => cfg.provider_strategic.trim(),
    };
    let id_lower = id.to_lowercase();
    match id_lower.as_str() {
        "anthropic" => {
            let Some(ref api_key) = secrets.anthropic_api_key else {
                return Err(AiError::ProviderNotConfigured("ANTHROPIC_API_KEY".into()));
            };
            Ok(Arc::new(AnthropicProvider::from_settings(
                api_key.clone(),
                secrets.anthropic_base_url.clone(),
                secrets.anthropic_timeout_secs,
            )?))
        }
        "openai_compatible" | "openai_compatible_onprem" | "openai" | "vllm" | "tgi" => Ok(Arc::new(
            OpenAiCompatibleProvider::from_settings(
                secrets.openai_compat_base_url.clone(),
                secrets.onprem_api_key.clone(),
                secrets.openai_compat_headers_json.clone(),
                secrets.onprem_timeout_secs,
                secrets.onprem_max_in_flight,
            )?,
        )),
        "ollama" => Ok(Arc::new(OllamaProvider::from_settings(
            secrets.ollama_base_url.clone(),
            secrets.onprem_timeout_secs,
        )?)),
        _ => Err(AiError::UnknownProvider(id.to_string())),
    }
}
