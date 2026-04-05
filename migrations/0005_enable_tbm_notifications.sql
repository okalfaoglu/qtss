-- Enable TBM setup notifications via Telegram
INSERT INTO system_config (module, config_key, value) VALUES
  ('notify', 'notify_on_tbm_setup', '"true"'),
  ('notify', 'notify_on_tbm_channels', '"telegram"')
ON CONFLICT (module, config_key) DO UPDATE SET value = EXCLUDED.value;
