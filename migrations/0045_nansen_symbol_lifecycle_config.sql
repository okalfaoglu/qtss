-- 0045: Config seed for nansen_symbol_lifecycle (promote + disable).

INSERT INTO system_config (module, config_key, value, description)
VALUES
  ('worker', 'nansen_symbol_lifecycle_enabled',       'true',  'Enable Nansen-driven symbol promote/disable loop'),
  ('worker', 'nansen_symbol_lifecycle_tick_secs',     '3600',  'Tick interval for promote/disable sweep (1h)'),

  -- Promote settings
  ('worker', 'nansen_promote.min_score',              '80',    'Minimum nansen_setup_rows score to auto-promote'),
  ('worker', 'nansen_promote.max_active',             '20',    'Max non-retired/non-manual engine_symbols'),
  ('worker', 'nansen_promote.default_interval',       '"15m"', 'Default bar interval for promoted symbols'),
  ('worker', 'nansen_promote.lookback_hours',         '24',    'Hours to look back in nansen_setup_rows'),

  -- Disable settings
  ('worker', 'nansen_disable.grace_hours',            '48',    'Hours without data before disabling a symbol')
ON CONFLICT (module, config_key) DO NOTHING;
