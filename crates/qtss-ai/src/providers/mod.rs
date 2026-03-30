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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResponse {
    pub text: String,
    pub model: String,
    pub provider_id: String,
}

#[async_trait]
pub trait AiCompletionProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;

    async fn complete(&self, req: &AiRequest) -> AiResult<AiResponse>;
}

/// Build the configured provider for a layer (`ai_engine_config.provider_*` + env URLs / keys).
pub fn provider_for_layer(
    cfg: &AiEngineConfig,
    layer: LayerKind,
) -> AiResult<Arc<dyn AiCompletionProvider>> {
    let id = match layer {
        LayerKind::Tactical => cfg.provider_tactical.trim(),
        LayerKind::Operational => cfg.provider_operational.trim(),
        LayerKind::Strategic => cfg.provider_strategic.trim(),
    };
    let id_lower = id.to_lowercase();
    match id_lower.as_str() {
        "anthropic" => Ok(Arc::new(AnthropicProvider::from_env()?)),
        "openai_compatible" | "openai_compatible_onprem" | "openai" | "vllm" | "tgi" => {
            Ok(Arc::new(OpenAiCompatibleProvider::from_env()?))
        }
        "ollama" => Ok(Arc::new(OllamaProvider::from_env()?)),
        _ => Err(AiError::UnknownProvider(id.to_string())),
    }
}
