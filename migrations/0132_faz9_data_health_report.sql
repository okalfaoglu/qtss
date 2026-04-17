-- 0132_faz9_data_health_report.sql
--
-- Faz 9 — Daily Data Health Report config keys.
--
-- A scheduled worker loop gathers system-wide stats every N hours and
-- enqueues a Telegram notification summarising data health.

SELECT _qtss_register_key(
    'health_report.enabled',
    'health',
    'health',
    'bool',
    'true'::jsonb,
    '',
    'Enable the periodic data health report.',
    'bool',
    false,
    'normal',
    ARRAY['health','faz9']
);

SELECT _qtss_register_key(
    'health_report.interval_hours',
    'health',
    'health',
    'int',
    '24'::jsonb,
    '',
    'Hours between health report runs.',
    'number',
    false,
    'normal',
    ARRAY['health','faz9']
);

SELECT _qtss_register_key(
    'health_report.channel',
    'health',
    'health',
    'string',
    '"telegram"'::jsonb,
    '',
    'Notification channel for the health report (telegram, email, etc).',
    'text',
    false,
    'normal',
    ARRAY['health','faz9']
);
