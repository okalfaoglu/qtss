//! `ai_engine_config` shape + optional env overrides (FAZ 2, FAZ 8).

use serde::{Deserialize, Serialize};

/// Loaded from `app_config.key = 'ai_engine_config'` and merged with env (FAZ 2.7 / 8.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiEngineConfig {
    pub enabled: bool,
    pub tactical_layer_enabled: bool,
    pub operational_layer_enabled: bool,
    pub strategic_layer_enabled: bool,
    pub auto_approve_threshold: f64,
    pub auto_approve_enabled: bool,
    pub tactical_tick_secs: u64,
    pub operational_tick_secs: u64,
    pub strategic_tick_secs: u64,
    pub provider_tactical: String,
    pub provider_operational: String,
    pub provider_strategic: String,
    pub model_tactical: String,
    pub model_operational: String,
    pub model_strategic: String,
    pub max_tokens_tactical: u32,
    pub max_tokens_operational: u32,
    pub max_tokens_strategic: u32,
    pub decision_ttl_secs: u64,
    pub require_min_confidence: f64,
    /// Target locale hint for prompts / reasoning (FAZ 9.5 placeholder).
    #[serde(default)]
    pub output_locale: Option<String>,
}

impl AiEngineConfig {
    pub fn default_disabled() -> Self {
        Self {
            enabled: false,
            tactical_layer_enabled: true,
            operational_layer_enabled: true,
            strategic_layer_enabled: false,
            auto_approve_threshold: 0.85,
            auto_approve_enabled: false,
            tactical_tick_secs: 900,
            operational_tick_secs: 120,
            strategic_tick_secs: 86_400,
            provider_tactical: "anthropic".into(),
            provider_operational: "anthropic".into(),
            provider_strategic: "anthropic".into(),
            model_tactical: "claude-haiku-4-5-20251001".into(),
            model_operational: "claude-haiku-4-5-20251001".into(),
            model_strategic: "claude-sonnet-4-20250514".into(),
            max_tokens_tactical: 1024,
            max_tokens_operational: 512,
            max_tokens_strategic: 4096,
            decision_ttl_secs: 1800,
            require_min_confidence: 0.60,
            output_locale: None,
        }
    }

    /// Env takes precedence over DB fields where set (operational toggles without admin UI).
    pub fn merge_env_overrides(&mut self) {
        if env_truthy("QTSS_AI_ENABLED") {
            self.enabled = true;
        }
        if env_truthy("QTSS_AI_DISABLED") {
            self.enabled = false;
        }
        if let Some(v) = env_u64("QTSS_AI_TACTICAL_TICK_SECS") {
            self.tactical_tick_secs = v;
        }
        if let Some(v) = env_u64("QTSS_AI_OPERATIONAL_TICK_SECS") {
            self.operational_tick_secs = v;
        }
        if let Some(v) = env_u64("QTSS_AI_STRATEGIC_TICK_SECS") {
            self.strategic_tick_secs = v;
        }
        if env_truthy("QTSS_AI_AUTO_APPROVE_ENABLED") {
            self.auto_approve_enabled = true;
        }
        if env_falsy("QTSS_AI_AUTO_APPROVE_ENABLED") {
            self.auto_approve_enabled = false;
        }
        if let Some(v) = env_f64("QTSS_AI_AUTO_APPROVE_THRESHOLD") {
            self.auto_approve_threshold = v;
        }
        if let Some(v) = env_f64("QTSS_AI_MIN_CONFIDENCE") {
            self.require_min_confidence = v;
        }
        if let Some(s) = env_string("QTSS_AI_PROVIDER_TACTICAL") {
            self.provider_tactical = s;
        }
        if let Some(s) = env_string("QTSS_AI_PROVIDER_OPERATIONAL") {
            self.provider_operational = s;
        }
        if let Some(s) = env_string("QTSS_AI_PROVIDER_STRATEGIC") {
            self.provider_strategic = s;
        }
        if let Some(s) = env_string("QTSS_AI_MODEL_TACTICAL") {
            self.model_tactical = s;
        }
        if let Some(s) = env_string("QTSS_AI_MODEL_OPERATIONAL") {
            self.model_operational = s;
        }
        if let Some(s) = env_string("QTSS_AI_MODEL_STRATEGIC") {
            self.model_strategic = s;
        }
        if let Some(s) = env_string("QTSS_AI_OUTPUT_LOCALE") {
            self.output_locale = Some(s);
        }
        if env_truthy("QTSS_AI_STRATEGIC_ENABLED") {
            self.strategic_layer_enabled = true;
        }
        if env_falsy("QTSS_AI_STRATEGIC_ENABLED") {
            self.strategic_layer_enabled = false;
        }
    }
}

fn env_string(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn env_u64(key: &str) -> Option<u64> {
    env_string(key).and_then(|s| s.parse().ok())
}

fn env_f64(key: &str) -> Option<f64> {
    env_string(key).and_then(|s| s.parse().ok())
}

fn env_truthy(key: &str) -> bool {
    match std::env::var(key) {
        Ok(s) => {
            let x = s.trim().to_lowercase();
            matches!(x.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}

fn env_falsy(key: &str) -> bool {
    match std::env::var(key) {
        Ok(s) => {
            let x = s.trim().to_lowercase();
            matches!(x.as_str(), "0" | "false" | "no" | "off")
        }
        Err(_) => false,
    }
}
