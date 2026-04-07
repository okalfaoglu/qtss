-- Intake → notify_outbox channel list (primary: system_config; env QTSS_INTAKE_PLAYBOOK_NOTIFY_CHANNELS is fallback).

INSERT INTO system_config (module, config_key, value, description, is_secret)
VALUES (
        'worker',
        'intake_playbook_notify_channels',
        '{"value":"telegram"}'::jsonb,
        'Comma-separated channels for intake_playbook_engine notify_outbox rows (telegram, webhook, …)',
        false
    )
ON CONFLICT (module, config_key) DO NOTHING;
