-- 0042: Seed system_config with Nansen credit monitor tunables.

INSERT INTO system_config (module, config_key, value, description)
VALUES
  ('monitoring', 'nansen_credit_check_enabled',       'true',    'Enable/disable Nansen credit monitor loop'),
  ('monitoring', 'nansen_credit_check_tick_secs',      '900',    'Check interval in seconds (default 15 min)'),
  ('monitoring', 'nansen_credit_warn_pct',             '20.0',   'Warning threshold: alert when remaining < this %'),
  ('monitoring', 'nansen_credit_critical_pct',         '5.0',    'Critical threshold: urgent alert when remaining < this %'),
  ('monitoring', 'nansen_credit_alert_cooldown_secs',  '21600',  'Dedupe window per severity level (default 6h)'),
  ('monitoring', 'nansen_credit_alert_channels',       '"telegram"', 'Notification channels (comma-separated: telegram, webhook)')
ON CONFLICT (module, config_key) DO NOTHING;
