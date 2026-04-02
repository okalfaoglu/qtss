//! Worker-facing config resolution: `system_config` JSON + env + `QTSS_CONFIG_ENV_OVERRIDES` (FAZ 11.7).

use rust_decimal::Decimal;
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use std::str::FromStr;

use crate::system_config::SystemConfigRepository;

/// Reads `secs`, `tick_secs`, or a bare integer from a `system_config.value` JSON object.
pub fn tick_secs_from_config_value(value: &JsonValue) -> Option<u64> {
    value
        .get("secs")
        .or_else(|| value.get("tick_secs"))
        .and_then(|x| x.as_u64())
        .or_else(|| value.as_u64())
}

fn bool_from_config_value(value: &JsonValue) -> Option<bool> {
    value
        .get("enabled")
        .and_then(|x| x.as_bool())
        .or_else(|| value.as_bool())
}

fn clamp_tick(raw: u64, min_secs: u64) -> u64 {
    raw.max(min_secs)
}

fn string_from_config_value(value: &JsonValue) -> Option<String> {
    let raw = value.get("value").or(if value.is_string() {
        Some(value)
    } else {
        None
    });
    if let Some(x) = raw {
        if let Some(s) = x.as_str() {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        } else if let Some(n) = x.as_f64() {
            return Some(n.to_string());
        } else if let Some(u) = x.as_u64() {
            return Some(u.to_string());
        } else if let Some(i) = x.as_i64() {
            return Some(i.to_string());
        }
    }
    value
        .as_str()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn f64_from_config_value(value: &JsonValue) -> Option<f64> {
    value
        .get("value")
        .and_then(|x| {
            x.as_f64()
                .or_else(|| x.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .or_else(|| value.as_f64())
}

fn u64_from_config_value(value: &JsonValue) -> Option<u64> {
    let from_num = |v: &JsonValue| {
        v.as_u64()
            .or_else(|| v.as_i64().filter(|&i| i >= 0).map(|i| i as u64))
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    };
    value
        .get("value")
        .and_then(from_num)
        .or_else(|| value.get("bars").and_then(from_num))
        .or_else(|| value.get("window").and_then(from_num))
        .or_else(|| from_num(value))
}

/// JSON `value` field (string or number) → [`Decimal`].
pub fn decimal_from_config_value(value: &JsonValue) -> Option<Decimal> {
    value
        .get("value")
        .and_then(|x| {
            if let Some(s) = x.as_str() {
                return Decimal::from_str(s.trim()).ok();
            }
            x.as_f64()
                .and_then(Decimal::from_f64_retain)
        })
        .or_else(|| {
            value
                .as_str()
                .and_then(|s| Decimal::from_str(s.trim()).ok())
        })
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

/// Resolution order: if `QTSS_CONFIG_ENV_OVERRIDES=1`, matching `env_key` wins; else `system_config`; else `env_key`; else `default_enabled`.
pub async fn resolve_worker_enabled_flag(
    pool: &PgPool,
    module: &str,
    config_key: &str,
    env_key: &str,
    default_enabled: bool,
) -> bool {
    if qtss_common::env_overrides_enabled() {
        if let Ok(s) = std::env::var(env_key) {
            let t = s.trim().to_lowercase();
            if matches!(t.as_str(), "1" | "true" | "yes" | "on") {
                return true;
            }
            if matches!(t.as_str(), "0" | "false" | "no" | "off") {
                return false;
            }
        }
    }

    let repo = SystemConfigRepository::new(pool.clone());
    if let Ok(Some(row)) = repo.get(module, config_key).await {
        if let Some(b) = bool_from_config_value(&row.value) {
            return b;
        }
    }

    if let Ok(s) = std::env::var(env_key) {
        let t = s.trim().to_lowercase();
        if matches!(t.as_str(), "1" | "true" | "yes" | "on") {
            return true;
        }
        if matches!(t.as_str(), "0" | "false" | "no" | "off") {
            return false;
        }
    }

    default_enabled
}

/// Resolution order: if `QTSS_CONFIG_ENV_OVERRIDES=1`, matching `env_key` wins; else `system_config`; else `env_key`; else `default_f64`.
pub async fn resolve_system_f64(
    pool: &PgPool,
    module: &str,
    config_key: &str,
    env_key: &str,
    default_f64: f64,
) -> f64 {
    if qtss_common::env_overrides_enabled() {
        if let Ok(s) = std::env::var(env_key) {
            let t = s.trim();
            if !t.is_empty() {
                if let Ok(v) = t.parse::<f64>() {
                    return v;
                }
            }
        }
    }

    let repo = SystemConfigRepository::new(pool.clone());
    if let Ok(Some(row)) = repo.get(module, config_key).await {
        if let Some(v) = f64_from_config_value(&row.value) {
            if v.is_finite() {
                return v;
            }
        }
    }

    std::env::var(env_key)
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(default_f64)
}

/// Same precedence as [`resolve_system_f64`]: JSON (`value`, `bars`, `window`, or bare number), then env, then default — clamped to \[`min_u64`, `max_u64`\].
pub async fn resolve_system_u64(
    pool: &PgPool,
    module: &str,
    config_key: &str,
    env_key: &str,
    default_u64: u64,
    min_u64: u64,
    max_u64: u64,
) -> u64 {
    let clamp = |u: u64| u.clamp(min_u64, max_u64);

    if qtss_common::env_overrides_enabled() {
        if let Ok(s) = std::env::var(env_key) {
            let t = s.trim();
            if !t.is_empty() {
                if let Ok(u) = t.parse::<u64>() {
                    return clamp(u);
                }
            }
        }
    }

    let repo = SystemConfigRepository::new(pool.clone());
    if let Ok(Some(row)) = repo.get(module, config_key).await {
        if let Some(u) = u64_from_config_value(&row.value) {
            return clamp(u);
        }
    }

    std::env::var(env_key)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(clamp)
        .unwrap_or_else(|| clamp(default_u64))
}

/// Same precedence as [`resolve_system_string`], parses [`Decimal`].
pub async fn resolve_system_decimal(
    pool: &PgPool,
    module: &str,
    config_key: &str,
    env_key: &str,
    default_decimal: Decimal,
) -> Decimal {
    if qtss_common::env_overrides_enabled() {
        if let Ok(s) = std::env::var(env_key) {
            let t = s.trim();
            if !t.is_empty() {
                if let Ok(d) = Decimal::from_str(t) {
                    return d;
                }
            }
        }
    }

    let repo = SystemConfigRepository::new(pool.clone());
    if let Ok(Some(row)) = repo.get(module, config_key).await {
        if let Some(d) = decimal_from_config_value(&row.value) {
            return d;
        }
    }

    std::env::var(env_key)
        .ok()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .unwrap_or(default_decimal)
}

/// Resolution order: if `QTSS_CONFIG_ENV_OVERRIDES=1`, matching `env_key` wins; else `system_config`; else `env_key`; else `default_value`.
pub async fn resolve_system_string(
    pool: &PgPool,
    module: &str,
    config_key: &str,
    env_key: &str,
    default_value: &str,
) -> String {
    if qtss_common::env_overrides_enabled() {
        if let Ok(s) = std::env::var(env_key) {
            let t = s.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }

    let repo = SystemConfigRepository::new(pool.clone());
    if let Ok(Some(row)) = repo.get(module, config_key).await {
        if let Some(s) = string_from_config_value(&row.value) {
            return s;
        }
    }

    std::env::var(env_key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_value.to_string())
}

/// Resolution order: if `QTSS_CONFIG_ENV_OVERRIDES=1`, matching `env_key` wins; else `system_config`; else `env_key`; else `default_values_csv`.
pub async fn resolve_system_csv(
    pool: &PgPool,
    module: &str,
    config_key: &str,
    env_key: &str,
    default_values_csv: &str,
) -> Vec<String> {
    let raw = resolve_system_string(pool, module, config_key, env_key, default_values_csv).await;
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Normalizes user-facing locale hints to `en` or `tr` (worker notify default).
pub fn normalize_notify_locale_code(raw: &str) -> Option<&'static str> {
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
            if let Some(c) = normalize_notify_locale_code(&s) {
                return c.to_string();
            }
        }
    }

    let repo = SystemConfigRepository::new(pool.clone());
    if let Ok(Some(row)) = repo.get("worker", "notify_default_locale").await {
        if let Some(c) = row.value.get("code").and_then(|x| x.as_str()) {
            if let Some(n) = normalize_notify_locale_code(c) {
                return n.to_string();
            }
        }
    }

    if let Ok(s) = std::env::var(ENV_KEY) {
        if let Some(c) = normalize_notify_locale_code(&s) {
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

    #[test]
    fn normalize_notify_locale_tr_variants() {
        assert_eq!(normalize_notify_locale_code("tr"), Some("tr"));
        assert_eq!(normalize_notify_locale_code("TR"), Some("tr"));
        assert_eq!(normalize_notify_locale_code("tr-TR"), Some("tr"));
    }

    #[test]
    fn normalize_notify_locale_en_variants() {
        assert_eq!(normalize_notify_locale_code("en"), Some("en"));
        assert_eq!(normalize_notify_locale_code("EN-US"), Some("en"));
    }

    #[test]
    fn normalize_notify_locale_unknown_none() {
        assert_eq!(normalize_notify_locale_code(""), None);
        assert_eq!(normalize_notify_locale_code("de"), None);
    }
}
