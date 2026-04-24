//! `system_config` module `ai` + isteğe bağlı env (yalnızca `QTSS_CONFIG_ENV_OVERRIDES=1`).
//!
//! PR-SEC2: vault-first resolution. When `QTSS_SECRET_KEK_V*` env vars
//! are present the loader spins up a `VaultReader` and tries the
//! `secrets_vault` table first; a miss falls back to the historical
//! `system_config` path with a warning + audit row. Rotating an API
//! key is then a `qtss-secret-cli put <name>` away — no code change,
//! no config-editor edit in plaintext.

use std::sync::Arc;

use qtss_secrets::{
    load_static_kek_from_env, KekProvider, PgSecretStore, VaultReader,
};
use sqlx::PgPool;
use tracing::{debug, warn};

use qtss_storage::{
    resolve_system_string, resolve_worker_tick_secs, SystemConfigRepository,
};

fn json_secret_value(v: &serde_json::Value) -> Option<String> {
    v.get("value")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn get_secret_trimmed_module(pool: &PgPool, module: &str, config_key: &str) -> Option<String> {
    let repo = SystemConfigRepository::new(pool.clone());
    let row = repo.get(module, config_key).await.ok()??;
    json_secret_value(&row.value)
}

// `get_secret_trimmed` used to be the only lookup path; after PR-SEC2
// everything routes through `resolve_secret_vault_first`, but the
// module-scoped helper is still useful for the telegram gemini path
// (which isn't yet vault-migrated). Keeping it private + prefixed
// so dead-code lint stays quiet if a future consumer drops out.
#[allow(dead_code)]
async fn get_secret_trimmed(pool: &PgPool, config_key: &str) -> Option<String> {
    get_secret_trimmed_module(pool, "ai", config_key).await
}

/// Try the vault first (if a KEK is available) and fall back to
/// `system_config` if the vault either isn't bootstrapped or the secret
/// hasn't been migrated yet. Every vault hit writes an audit row — the
/// fallback path also logs via `VaultReader::resolve`, so operators can
/// diff "vault hit" vs "config fallback" to track rollout progress.
///
/// The helper is cheap enough to call per-secret because `load_static_kek_from_env`
/// re-reads env each time. For startup-critical paths `AiProviderSecrets::load`
/// calls this a handful of times — no caching hoops needed.
async fn resolve_secret_vault_first(
    pool: &PgPool,
    name: &str,
    reason: &str,
    config_module: &str,
) -> Option<String> {
    // Vault path — only taken when a KEK is actually present. A missing
    // KEK is the no-vault dev default; it shouldn't be an error.
    if let Ok(kek) = load_static_kek_from_env() {
        let provider: Arc<dyn KekProvider> = Arc::new(kek);
        let store = Arc::new(PgSecretStore::new(pool.clone(), provider));
        let reader = VaultReader::new(pool.clone(), store, "qtss-ai");
        match reader.resolve_str(name, reason).await {
            Ok((value, src)) => {
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() {
                    debug!(%name, ?src, "vault returned empty string, trying config");
                } else {
                    return Some(trimmed);
                }
            }
            Err(e) => {
                warn!(%name, %e, "vault resolve failed — falling back to system_config");
            }
        }
    }
    // Fallback: original system_config read. Keeps zero regression
    // for installs that haven't rotated keys into the vault yet.
    get_secret_trimmed_module(pool, config_module, name).await
}

/// Inference uçları ve anahtarlar — `provider_for_layer` girdisi.
#[derive(Debug, Clone, Default)]
pub struct AiProviderSecrets {
    pub anthropic_api_key: Option<String>,
    pub anthropic_base_url: String,
    pub anthropic_timeout_secs: u64,
    /// Optional dedicated key; if empty, `telegram_setup_analysis.gemini_api_key` is used (same Google AI Studio key).
    pub gemini_api_key: Option<String>,
    pub gemini_api_root: String,
    pub gemini_timeout_secs: u64,
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
            gemini_api_root: resolve_system_string(
                pool,
                "ai",
                "gemini_api_root",
                "QTSS_AI_GEMINI_API_ROOT",
                "",
            )
            .await,
            gemini_timeout_secs: resolve_worker_tick_secs(
                pool,
                "ai",
                "gemini_timeout_secs",
                "QTSS_AI_GEMINI_TIMEOUT_SECS",
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

        // PR-SEC2: vault-first lookups. Each secret first tries the
        // encrypted vault; a miss transparently falls back to
        // system_config (with a warning + audit row logged by
        // VaultReader). `reason` values feed secret_access_log so
        // operators can tell *which* loop reached for the key.
        s.anthropic_api_key = resolve_secret_vault_first(
            pool,
            "anthropic_api_key",
            "ai.provider_secrets.load",
            "ai",
        )
        .await;
        s.onprem_api_key = resolve_secret_vault_first(
            pool,
            "onprem_api_key",
            "ai.provider_secrets.load",
            "ai",
        )
        .await;
        let ai_g =
            resolve_secret_vault_first(pool, "gemini_api_key", "ai.provider_secrets.load", "ai")
                .await;
        // Telegram module owns its own gemini_api_key row today; keep
        // the dual lookup until the Telegram consumer migration lands
        // in a later PR.
        let tg_g = get_secret_trimmed_module(pool, "telegram_setup_analysis", "gemini_api_key").await;
        s.gemini_api_key = match (
            ai_g.filter(|x| !x.is_empty()),
            tg_g.filter(|x| !x.is_empty()),
        ) {
            (Some(k), _) | (None, Some(k)) => Some(k),
            _ => None,
        };
        if s.gemini_api_root.trim().is_empty() {
            s.gemini_api_root = "https://generativelanguage.googleapis.com/v1beta".to_string();
        }

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
            if let Ok(k) = std::env::var("QTSS_AI_GEMINI_API_KEY").or_else(|_| std::env::var("GEMINI_API_KEY")) {
                let t = k.trim().to_string();
                if !t.is_empty() {
                    s.gemini_api_key = Some(t);
                }
            }
            if let Ok(u) = std::env::var("QTSS_AI_GEMINI_API_ROOT") {
                let t = u.trim();
                if !t.is_empty() {
                    s.gemini_api_root = t.to_string();
                }
            }
        }

        s
    }
}
