-- Optional: intake sweep → notify_outbox (worker `intake_playbook_engine`).
-- Still off until `{"enabled":true}` or `QTSS_INTAKE_PLAYBOOK_NOTIFY_ENABLED=1`.

INSERT INTO system_config (module, config_key, value, description, is_secret)
VALUES (
        'worker',
        'intake_playbook_notify_enabled',
        '{"enabled":false}'::jsonb,
        'Enqueue notify_outbox on market_mode change and ten_x_alert trigger',
        false
    )
ON CONFLICT (module, config_key) DO NOTHING;
