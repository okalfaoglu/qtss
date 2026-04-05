//! Resolved knobs for the setup-analysis webhook — **only** `system_config` (`telegram_setup_analysis` module).
//! No environment-variable reads for these keys (use Admin `system_config` or seed migration).

use std::collections::HashSet;

use serde_json::Value as JsonValue;
use sqlx::PgPool;

use qtss_storage::SystemConfigRepository;

const MODULE: &str = "telegram_setup_analysis";

/// Runtime configuration.
#[derive(Clone)]
pub struct ResolvedSetupAnalysisConfig {
    pub trigger_phrase: String,
    pub gemini_api_key: String,
    pub gemini_model: String,
    pub webhook_secret: String,
    pub max_buffer_turns: u32,
    pub buffer_ttl_secs: i64,
    allowed_chat_ids: HashSet<i64>,
}

impl std::fmt::Debug for ResolvedSetupAnalysisConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedSetupAnalysisConfig")
            .field("trigger_phrase", &self.trigger_phrase)
            .field("gemini_api_key", &"<redacted>")
            .field("gemini_model", &self.gemini_model)
            .field("webhook_secret", &"<redacted>")
            .field("max_buffer_turns", &self.max_buffer_turns)
            .field("buffer_ttl_secs", &self.buffer_ttl_secs)
            .field("allowlist_size", &self.allowed_chat_ids.len())
            .finish()
    }
}

impl ResolvedSetupAnalysisConfig {
    pub async fn load(pool: &PgPool) -> Self {
        let repo = SystemConfigRepository::new(pool.clone());

        let trigger_phrase = read_nonempty_string(&repo, "trigger_phrase", "QTSS_ANALIZ").await;
        let gemini_model = read_nonempty_string(&repo, "gemini_model", "gemini-2.5-flash").await;
        let webhook_secret = read_string_allow_empty(&repo, "webhook_secret").await;
        let gemini_api_key = read_string_allow_empty(&repo, "gemini_api_key").await;

        let max_buffer_turns: u32 = read_nonempty_string(&repo, "max_buffer_turns", "12")
            .await
            .parse()
            .unwrap_or(12)
            .clamp(1, 50);

        let buffer_ttl_secs: i64 = read_nonempty_string(&repo, "buffer_ttl_secs", "7200")
            .await
            .parse()
            .unwrap_or(7200)
            .clamp(300, 86400);

        let allowed_raw = read_string_allow_empty(&repo, "allowed_chat_ids").await;
        let allowed_chat_ids: HashSet<i64> = allowed_raw
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<i64>().ok())
            .collect();

        Self {
            trigger_phrase,
            gemini_api_key,
            gemini_model,
            webhook_secret,
            max_buffer_turns,
            buffer_ttl_secs,
            allowed_chat_ids,
        }
    }

    pub fn allowlist_restricts(&self) -> bool {
        !self.allowed_chat_ids.is_empty()
    }

    pub fn allowlist_size(&self) -> usize {
        self.allowed_chat_ids.len()
    }

    pub fn webhook_enabled(&self) -> bool {
        !self.webhook_secret.trim().is_empty()
    }

    pub fn gemini_configured(&self) -> bool {
        !self.gemini_api_key.trim().is_empty()
    }

    pub fn chat_allowed(&self, chat_id: i64) -> bool {
        if self.allowed_chat_ids.is_empty() {
            return true;
        }
        self.allowed_chat_ids.contains(&chat_id)
    }

    /// Trigger if the whole message (trimmed) equals the phrase, or starts with `phrase + ' '` / `phrase + '\n'`.
    pub fn is_trigger_message(&self, text: Option<&str>) -> bool {
        let Some(t) = text.map(str::trim).filter(|s| !s.is_empty()) else {
            return false;
        };
        let p = self.trigger_phrase.trim();
        if p.is_empty() {
            return false;
        }
        t == p
            || t.starts_with(&format!("{p} "))
            || t.starts_with(&format!("{p}\n"))
            || t.starts_with(&format!("{p}\r\n"))
    }

    /// Extra user instructions after the trigger on the same message.
    pub fn strip_trigger_prefix<'a>(&self, text: Option<&'a str>) -> Option<&'a str> {
        let t = text?.trim();
        let p = self.trigger_phrase.trim();
        if t == p {
            return None;
        }
        for sep in [" ", "\n", "\r\n"] {
            let pref = format!("{p}{sep}");
            if let Some(rest) = t.strip_prefix(&pref) {
                return Some(rest);
            }
        }
        None
    }
}

fn json_value_string(value: &JsonValue) -> Option<String> {
    let v = value.get("value").unwrap_or(value);
    if let Some(s) = v.as_str() {
        return Some(s.trim().to_string());
    }
    if let Some(n) = v.as_i64() {
        return Some(n.to_string());
    }
    if let Some(n) = v.as_u64() {
        return Some(n.to_string());
    }
    if let Some(f) = v.as_f64() {
        return Some(f.to_string());
    }
    None
}

async fn read_string_allow_empty(repo: &SystemConfigRepository, key: &str) -> String {
    match repo.get(MODULE, key).await {
        Ok(Some(row)) => json_value_string(&row.value).unwrap_or_default(),
        _ => String::new(),
    }
}

async fn read_nonempty_string(repo: &SystemConfigRepository, key: &str, default: &str) -> String {
    match repo.get(MODULE, key).await {
        Ok(Some(row)) => {
            if let Some(s) = json_value_string(&row.value) {
                let t = s.trim();
                if !t.is_empty() {
                    return t.to_string();
                }
            }
            default.to_string()
        }
        _ => default.to_string(),
    }
}
