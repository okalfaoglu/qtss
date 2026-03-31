-- Worker feature flags (UI checkbox support) — safe defaults.

INSERT INTO system_config (module, config_key, value, description, is_secret)
VALUES
  ('worker', 'nansen_enabled', '{"enabled": false}', 'Enable Nansen HTTP loops (prevents credit burn when false).', false),
  ('worker', 'external_fetch_enabled', '{"enabled": true}', 'Enable external_data_sources HTTP engines.', false)
ON CONFLICT (module, config_key) DO NOTHING;

