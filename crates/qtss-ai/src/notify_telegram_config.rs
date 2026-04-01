//! `system_config` satırlarından Telegram (`notify.telegram_*`) + mevcut `dispatcher_config` / env birleşimi.

use qtss_notify::{config::TelegramConfig, NotifyConfig};
use qtss_storage::SystemConfigRepository;
use sqlx::PgPool;

fn value_from_row_json(value: &serde_json::Value) -> Option<String> {
    value
        .get("value")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// `notify.telegram_bot_token` + `notify.telegram_chat_id` varsa `cfg.telegram` alanını **üzerine yazar** (DB öncelikli).
pub async fn apply_notify_telegram_system_config(
    pool: &PgPool,
    cfg: &mut NotifyConfig,
) -> Result<(), qtss_storage::StorageError> {
    let repo = SystemConfigRepository::new(pool.clone());
    let token = repo
        .get("notify", "telegram_bot_token")
        .await?
        .and_then(|r| value_from_row_json(&r.value));
    let chat = repo
        .get("notify", "telegram_chat_id")
        .await?
        .and_then(|r| value_from_row_json(&r.value));
    if let (Some(bot_token), Some(chat_id)) = (token, chat) {
        cfg.telegram = Some(TelegramConfig {
            bot_token,
            chat_id,
        });
    }
    Ok(())
}

/// Worker / API ile aynı birleşik bildirim yapılandırması: `dispatcher_config` → env (isteğe bağlı) → `telegram_*` satırları.
pub async fn load_notify_config_merged(pool: &PgPool) -> NotifyConfig {
    let mut ncfg = match SystemConfigRepository::get_value_json(pool, "notify", "dispatcher_config").await {
        Ok(Some(v)) => NotifyConfig::from_system_config_value(&v),
        Ok(None) | Err(_) => {
            if qtss_common::env_overrides_enabled() {
                NotifyConfig::from_env()
            } else {
                NotifyConfig::default()
            }
        }
    };
    if let Err(e) = apply_notify_telegram_system_config(pool, &mut ncfg).await {
        tracing::warn!(error = %e, "apply_notify_telegram_system_config failed");
    }
    ncfg
}
