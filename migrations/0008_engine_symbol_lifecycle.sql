-- 0008: engine_symbols lifecycle state machine + auto-promote / lifecycle manager config
--
-- States: manual | promoted | analyzing | ready | trading | closing | cooldown | retired
-- 'manual' = existing rows not managed by lifecycle automation

ALTER TABLE engine_symbols
  ADD COLUMN IF NOT EXISTS lifecycle_state TEXT NOT NULL DEFAULT 'manual';

UPDATE engine_symbols SET lifecycle_state = 'manual'
WHERE lifecycle_state IS NULL OR lifecycle_state = '';

COMMENT ON COLUMN engine_symbols.lifecycle_state IS
  'manual | promoted | analyzing | ready | trading | closing | cooldown | retired';

CREATE INDEX IF NOT EXISTS idx_engine_symbols_lifecycle
  ON engine_symbols (lifecycle_state) WHERE lifecycle_state NOT IN ('retired', 'manual');

INSERT INTO system_config (module, key, value_json)
VALUES
  ('worker', 'intake_auto_promote_enabled',
   '{"enabled": false}'::jsonb),
  ('worker', 'intake_auto_promote_tick_secs',
   '{"secs": 120}'::jsonb),
  ('worker', 'intake_auto_promote_min_confidence',
   '{"value": 60}'::jsonb),
  ('worker', 'intake_auto_promote_playbooks',
   '{"value": "elite_long,elite_short,ten_x_alert,institutional_accumulation,institutional_exit"}'::jsonb),
  ('worker', 'intake_auto_promote_max_active',
   '{"value": 20}'::jsonb),
  ('worker', 'intake_auto_promote_default_interval',
   '{"value": "15m"}'::jsonb),
  ('worker', 'lifecycle_manager_enabled',
   '{"enabled": false}'::jsonb),
  ('worker', 'lifecycle_manager_tick_secs',
   '{"secs": 300}'::jsonb),
  ('worker', 'lifecycle_cooldown_hours',
   '{"value": 24}'::jsonb),
  ('worker', 'lifecycle_retire_stale_hours',
   '{"value": 48}'::jsonb)
ON CONFLICT (module, key) DO NOTHING;
