-- FAZ 9.3 — user locale preference; FAZ 11.7 — worker tick seeds (non-secret).

ALTER TABLE users ADD COLUMN IF NOT EXISTS preferred_locale TEXT;

COMMENT ON COLUMN users.preferred_locale IS 'BCP47-style language tag: en, tr, or NULL for default.';

INSERT INTO system_config (module, config_key, value, description)
VALUES
    (
        'worker',
        'notify_outbox_tick_secs',
        '{"secs":10}'::jsonb,
        'notify_outbox consumer poll interval (seconds); env QTSS_NOTIFY_OUTBOX_TICK_SECS when QTSS_CONFIG_ENV_OVERRIDES=1 overrides DB.'
    ),
    (
        'worker',
        'pnl_rollup_tick_secs',
        '{"secs":300}'::jsonb,
        'PnL rollup rebuild interval (seconds); env QTSS_PNL_ROLLUP_TICK_SECS; min 60 in worker.'
    ),
    (
        'worker',
        'notify_default_locale',
        '{"code":"tr"}'::jsonb,
        'Default locale for worker bilingual notify copy (en|tr); env QTSS_NOTIFY_DEFAULT_LOCALE.'
    )
ON CONFLICT (module, config_key) DO NOTHING;
