-- Engine analysis notify toggles (`qtss-analysis` `engine_loop`): DB-first, env fallback (`QTSS_*`), `QTSS_CONFIG_ENV_OVERRIDES=1` env wins.
INSERT INTO system_config (module, config_key, value, description, is_secret) VALUES
('notify', 'notify_on_sweep', '{"enabled":false}', 'Sweep edge Telegram/webhook from engine loop', false),
('notify', 'notify_on_sweep_channels', '{"value":"webhook"}', 'Comma channel list for sweep notify', false),
('notify', 'notify_on_range_events', '{"enabled":false}', 'Trading range setup + range_signal_events notify', false),
('notify', 'notify_on_range_events_channels', '{"value":"telegram"}', 'Comma channel list for range events', false)
ON CONFLICT (module, config_key) DO NOTHING;
