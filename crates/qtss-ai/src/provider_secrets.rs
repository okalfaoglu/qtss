//! `system_config` module `ai` + isteğe bağlı env (yalnızca `QTSS_CONFIG_ENV_OVERRIDES=1`).

use sqlx::PgPool;

use qtss_storage::{
    resolve_system_string, resolve_worker_tick_secs, SystemConfigRepository,
};

fn json_secret_value(v: &serde_json::Value) -> Option<String> {
    v.get("value")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn get_secret_trimmed(pool: &PgPool, config_key: &str) -> Option<String> {
    let repo = SystemConfigRepository::new(pool.clone());
    let row = repo.get("ai", config_key).await.ok()??;
    json_secret_value(&row.value)
}

/// Inference uçları ve anahtarlar — `provider_for_layer` girdisi.
#[derive(Debug, Clone, Default)]
pub struct AiProviderSecrets {
    pub anthropic_api_key: Option<String>,
    pub anthropic_base_url: String,
    pub anthropic_timeout_secs: u64,
    pub ollama_base_url: String,
    pub openai_compat_base_url: String,
    pub openai_compat_headers_json: Option<String>,
    pub onprem_timeout_secs: u64,
    pub onprem_max_in_flight: usize,
    pub onprem_api_key: Option<String>,
}

impl AiProviderSecrets {
    pub async fn load(pool: &PgPool) -> Self {
        let mut s = Self {
            anthropic_base_url: resolve_system_string(
                pool,
                "ai",
                "anthropic_base_url",
                "ANTHROPIC_BASE_URL",
                "https://api.anthropic.com",
            )
            .await,
            anthropic_timeout_secs: resolve_worker_tick_secs(
                pool,
                "ai",
                "anthropic_timeout_secs",
                "QTSS_AI_ANTHROPIC_TIMEOUT_SECS",
                120,
                30,
            )
            .await,
            ollama_base_url: resolve_system_string(
                pool,
                "ai",
                "ollama_base_url",
                "QTSS_AI_OLLAMA_BASE_URL",
                "http://127.0.0.1:11434",
            )
            .await,
            openai_compat_base_url: resolve_system_string(
                pool,
                "ai",
                "openai_compat_base_url",
                "QTSS_AI_OPENAI_COMPAT_BASE_URL",
                "",
            )
            .await,
            openai_compat_headers_json: {
                let raw = resolve_system_string(
                    pool,
                    "ai",
                    "openai_compat_headers_json",
                    "QTSS_AI_OPENAI_COMPAT_HEADERS_JSON",
                    "",
                )
                .await;
                if raw.trim().is_empty() {
                    None
                } else {
                    Some(raw)
                }
            },
            onprem_timeout_secs: resolve_worker_tick_secs(
                pool,
                "ai",
                "onprem_timeout_secs",
                "QTSS_AI_ONPREM_TIMEOUT_SECS",
                180,
                30,
            )
            .await,
            onprem_max_in_flight: resolve_system_string(
                pool,
                "ai",
                "onprem_max_in_flight",
                "QTSS_AI_ONPREM_MAX_IN_FLIGHT",
                "4",
            )
            .await
            .parse()
            .unwrap_or(4)
            .max(1),
            ..Default::default()
        };

        s.anthropic_api_key = get_secret_trimmed(pool, "anthropic_api_key").await;
        s.onprem_api_key = get_secret_trimmed(pool, "onprem_api_key").await;

        if qtss_common::env_overrides_enabled() {
            if let Ok(k) = std::env::var("ANTHROPIC_API_KEY").or_else(|_| std::env::var("QTSS_AI_ANTHROPIC_API_KEY")) {
                let t = k.trim().to_string();
                if !t.is_empty() {
                    s.anthropic_api_key = Some(t);
                }
            }
            if let Ok(u) = std::env::var("ANTHROPIC_BASE_URL").or_else(|_| std::env::var("QTSS_AI_ANTHROPIC_BASE_URL")) {
                let t = u.trim();
                if !t.is_empty() {
                    s.anthropic_base_url = t.to_string();
                }
            }
            if let Ok(u) = std::env::var("QTSS_AI_OLLAMA_BASE_URL").or_else(|_| std::env::var("OLLAMA_HOST")) {
                let t = u.trim();
                if !t.is_empty() {
                    s.ollama_base_url = t.to_string();
                }
            }
            if let Ok(u) = std::env::var("QTSS_AI_OPENAI_COMPAT_BASE_URL").or_else(|_| std::env::var("OPENAI_BASE_URL")) {
                let t = u.trim();
                if !t.is_empty() {
                    s.openai_compat_base_url = t.to_string();
                }
            }
            if let Ok(h) = std::env::var("QTSS_AI_OPENAI_COMPAT_HEADERS_JSON") {
                let t = h.trim();
                if !t.is_empty() {
                    s.openai_compat_headers_json = Some(t.to_string());
                }
            }
            if let Ok(k) = std::env::var("QTSS_AI_ONPREM_API_KEY").or_else(|_| std::env::var("OPENAI_API_KEY")) {
                let t = k.trim().to_string();
                if !t.is_empty() {
                    s.onprem_api_key = Some(t);
                }
            }
        }

        s
    }
}
