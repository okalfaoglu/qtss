-- Telegram setup-analysis webhook + Gemini: all settings live in `system_config` (module `telegram_setup_analysis`).
-- Fill `gemini_api_key` and `webhook_secret` via Admin UI or API; mark as secret.

INSERT INTO system_config (module, config_key, value, schema_version, description, is_secret)
VALUES
    (
        'telegram_setup_analysis',
        'trigger_phrase',
        '{"value":"QTSS_ANALIZ"}'::jsonb,
        1,
        'User sends this phrase (alone or with a trailing note) to flush the queue and run analysis.',
        false
    ),
    (
        'telegram_setup_analysis',
        'gemini_model',
        '{"value":"gemini-2.0-flash"}'::jsonb,
        1,
        'Gemini model id for generateContent (Google AI).',
        false
    ),
    (
        'telegram_setup_analysis',
        'webhook_secret',
        '{"value":""}'::jsonb,
        1,
        'Path secret for POST /telegram/setup-analysis/{secret}. Non-empty enables the webhook.',
        true
    ),
    (
        'telegram_setup_analysis',
        'gemini_api_key',
        '{"value":""}'::jsonb,
        1,
        'Google AI Studio / Gemini API key.',
        true
    ),
    (
        'telegram_setup_analysis',
        'max_buffer_turns',
        '{"value":"12"}'::jsonb,
        1,
        'Max queued items per chat (1–50).',
        false
    ),
    (
        'telegram_setup_analysis',
        'buffer_ttl_secs',
        '{"value":"7200"}'::jsonb,
        1,
        'Drop stale queue entries older than this many seconds (300–86400).',
        false
    ),
    (
        'telegram_setup_analysis',
        'allowed_chat_ids',
        '{"value":""}'::jsonb,
        1,
        'Optional comma-separated Telegram chat ids; empty = allow all chats.',
        false
    )
ON CONFLICT (module, config_key) DO NOTHING;
