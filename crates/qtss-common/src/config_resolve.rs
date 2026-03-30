//! Bootstrap and env override helpers for central config (FAZ 11.5 — thin layer).
//!
//! Full `system_config` + DB merge lives in `qtss-storage`; this module only centralizes
//! the **disaster-recovery** flag: when `QTSS_CONFIG_ENV_OVERRIDES=1`, callers may prefer
//! explicit environment values over DB (see `docs/CONFIG_REGISTRY.md`).

/// When true, runtime may treat matching `QTSS_*` env vars as overrides to DB-backed config.
pub fn env_overrides_enabled() -> bool {
    match std::env::var("QTSS_CONFIG_ENV_OVERRIDES") {
        Ok(s) => {
            let x = s.trim().to_lowercase();
            matches!(x.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}

/// Returns `std::env::var(key)` only if [`env_overrides_enabled`] is true and the value is non-empty.
pub fn env_override(key: &str) -> Option<String> {
    if !env_overrides_enabled() {
        return None;
    }
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_clean_env(f: impl FnOnce()) {
        let _g = ENV_LOCK.lock().expect("env test lock");
        let keys = ["QTSS_CONFIG_ENV_OVERRIDES", "QTSS_CONFIG_RESOLVE_TEST_KEY"];
        let saved: Vec<(String, Result<String, std::env::VarError>)> = keys
            .iter()
            .map(|k| (k.to_string(), std::env::var(k)))
            .collect();
        f();
        for (k, prev) in saved {
            match prev {
                Ok(v) => std::env::set_var(&k, v),
                Err(std::env::VarError::NotPresent) => std::env::remove_var(&k),
                Err(std::env::VarError::NotUnicode(_)) => std::env::remove_var(&k),
            }
        }
    }

    #[test]
    fn env_overrides_disabled_when_unset_or_off() {
        with_clean_env(|| {
            std::env::remove_var("QTSS_CONFIG_ENV_OVERRIDES");
            assert!(!env_overrides_enabled());
            std::env::set_var("QTSS_CONFIG_ENV_OVERRIDES", "0");
            assert!(!env_overrides_enabled());
            std::env::set_var("QTSS_CONFIG_ENV_OVERRIDES", "false");
            assert!(!env_overrides_enabled());
        });
    }

    #[test]
    fn env_overrides_enabled_for_truthy() {
        with_clean_env(|| {
            std::env::set_var("QTSS_CONFIG_ENV_OVERRIDES", "1");
            assert!(env_overrides_enabled());
            std::env::set_var("QTSS_CONFIG_ENV_OVERRIDES", "yes");
            assert!(env_overrides_enabled());
        });
    }

    #[test]
    fn env_override_returns_none_without_flag_even_if_var_set() {
        with_clean_env(|| {
            std::env::remove_var("QTSS_CONFIG_ENV_OVERRIDES");
            std::env::set_var("QTSS_CONFIG_RESOLVE_TEST_KEY", "x");
            assert_eq!(env_override("QTSS_CONFIG_RESOLVE_TEST_KEY"), None);
        });
    }

    #[test]
    fn env_override_returns_value_when_flag_on() {
        with_clean_env(|| {
            std::env::set_var("QTSS_CONFIG_ENV_OVERRIDES", "1");
            std::env::set_var("QTSS_CONFIG_RESOLVE_TEST_KEY", "  hello  ");
            assert_eq!(
                env_override("QTSS_CONFIG_RESOLVE_TEST_KEY").as_deref(),
                Some("hello")
            );
        });
    }
}
