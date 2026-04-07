-- Dedupe window for `intake_ten_x_alert` notify_outbox rows (per symbol, org_id NULL). 0 = off.

INSERT INTO system_config (module, config_key, value, description, is_secret)
VALUES (
        'worker',
        'intake_playbook_notify_ten_x_dedupe_secs',
        '{"secs":86400}'::jsonb,
        'Seconds: skip new intake_ten_x_alert outbox row if same symbol was queued recently',
        false
    )
ON CONFLICT (module, config_key) DO NOTHING;
