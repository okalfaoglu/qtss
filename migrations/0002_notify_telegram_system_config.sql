-- Default `system_config` rows for Telegram (`qtss-ai` `apply_notify_telegram_system_config`).
-- Empty `value` keeps Telegram off until admin fills token + chat_id (non-empty strings required).

INSERT INTO system_config (module, config_key, value, schema_version, description, is_secret)
VALUES
    (
        'notify',
        'telegram_bot_token',
        '{"value": ""}'::jsonb,
        1,
        'Telegram BotFather token',
        true
    ),
    (
        'notify',
        'telegram_chat_id',
        '{"value": ""}'::jsonb,
        1,
        'Telegram target chat/channel id',
        false
    )
ON CONFLICT (module, config_key) DO NOTHING;
