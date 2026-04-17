-- Faz 9.7.5 — Telegram lifecycle renderer toggle.
-- The TelegramLifecycleHandler is additive: enabled by default, but
-- silently no-ops when NotifyConfig has no Telegram section. Operators
-- can flip this off in the Config Editor if they want DB-only audit
-- without downstream Telegram fan-out.

SELECT _qtss_register_key(
    'telegram_lifecycle.enabled', 'notify', 'telegram_lifecycle',
    'bool', 'true'::jsonb, '',
    'Attach the TelegramLifecycleHandler to the SetupWatcher router. When false, lifecycle events are persisted but not shipped to Telegram.',
    'bool', true, 'normal', ARRAY['notify','faz97']
);
