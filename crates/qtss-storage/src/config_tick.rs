//! Worker-facing config resolution: `system_config` JSON + env + `QTSS_CONFIG_ENV_OVERRIDES` (FAZ 11.7).

use serde_json::Value as JsonValue;
use sqlx::PgPool;

use crate::system_config::SystemConfigRepository;

/// Reads `secs`, `tick_secs`, or a bare integer from a `system_config.value` JSON object.
pub fn tick_secs_from_config_value(value: &JsonValue) -> Option<u64> {
    value
        .get("secs")
        .or_else(|| value.get("tick_secs"))
        .and_then(|x| x.as_u64())
        .or_else(|| value.as_u64())
}

fn clamp_tick(raw: u64, min_secs: u64) -> u64 {
    raw.max(min_secs)
}

/// Resolution order: if `QTSS_CONFIG_ENV_OVERRIDES=1`, matching `env_key` wins; else `system_config`; else `env_key`; else `default_secs`.
pub async fn resolve_worker_tick_secs(
    pool: &PgPool,
    module: &str,
    config_key: &str,
    env_key: &str,
    default_secs: u64,
    min_secs: u64,
) -> u64 {
    if qtss_common::env_overrides_enabled() {
        if let Ok(s) = std::env::var(env_key) {
            let t = s.trim();
            if !t.is_empty() {
                if let Ok(u) = t.parse::<u64>() {
                    return clamp_tick(u, min_secs);
                }
            }
        }
    }

    let repo = SystemConfigRepository::new(pool.clone());
    if let Ok(Some(row)) = repo.get(module, config_key).await {
        if let Some(u) = tick_secs_from_config_value(&row.value) {
            return clamp_tick(u, min_secs);
        }
    }

    std::env::var(env_key)
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(|u| clamp_tick(u, min_secs))
        .unwrap_or_else(|| clamp_tick(default_secs, min_secs))
}

fn normalize_locale_code(raw: &str) -> Option<&'static str> {
    let t = raw.trim().to_lowercase();
    if t.starts_with("tr") {
        return Some("tr");
    }
    if t.starts_with("en") {
        return Some("en");
    }
    None
}

/// Default `tr` when unset or invalid. Same override precedence as tick resolution for the env key.
pub async fn resolve_notify_default_locale(pool: &PgPool) -> String {
    const ENV_KEY: &str = "QTSS_NOTIFY_DEFAULT_LOCALE";

    if qtss_common::env_overrides_enabled() {
        if let Ok(s) = std::env::var(ENV_KEY) {
            if let Some(c) = normalize_locale_code(&s) {
                return c.to_string();
            }
        }
    }

    let repo = SystemConfigRepository::new(pool.clone());
    if let Ok(Some(row)) = repo.get("worker", "notify_default_locale").await {
        if let Some(c) = row.value.get("code").and_then(|x| x.as_str()) {
            if let Some(n) = normalize_locale_code(c) {
                return n.to_string();
            }
        }
    }

    if let Ok(s) = std::env::var(ENV_KEY) {
        if let Some(c) = normalize_locale_code(&s) {
            return c.to_string();
        }
    }
    "tr".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tick_secs_from_object_and_scalar() {
        assert_eq!(tick_secs_from_config_value(&json!({"secs": 30})), Some(30));
        assert_eq!(tick_secs_from_config_value(&json!({"tick_secs": 5})), Some(5));
        assert_eq!(tick_secs_from_config_value(&json!(15)), Some(15));
        assert_eq!(tick_secs_from_config_value(&json!({})), None);
    }
}
