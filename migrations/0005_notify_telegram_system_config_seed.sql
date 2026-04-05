-- Optional Telegram credentials for `qtss-notify` / merged `load_notify_config_merged`.
-- Fill values via Admin or SQL; both marked secret. Chat id can be your user id, a group id, or
-- channel id (e.g. -100...) for default outbound notify; setup-analysis webhook still replies to
-- the chat_id from each update.

INSERT INTO system_config (module, config_key, value, schema_version, description, is_secret)
VALUES
    (
        'notify',
        'telegram_bot_token',
        '{"value":""}'::jsonb,
        1,
        'Telegram Bot API token (BotFather). Required for setup-analysis getFile/sendMessage.',
        true
    ),
    (
        'notify',
        'telegram_chat_id',
        '{"value":""}'::jsonb,
        1,
        'Default chat id for generic Telegram notifications (user / group / channel). Optional for webhook-only bot flows.',
        true
    )
ON CONFLICT (module, config_key) DO NOTHING;
