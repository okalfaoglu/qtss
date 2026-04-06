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

INSERT INTO system_config (module, config_key, value, description, is_secret)
VALUES
  ('worker', 'intake_auto_promote_enabled', '{"enabled": false}'::jsonb,
   'Enable intake_auto_promote_loop (auto-promote candidates to engine_symbols).', false),
  ('worker', 'intake_auto_promote_tick_secs', '{"secs": 120}'::jsonb,
   'Tick interval seconds for intake_auto_promote_loop.', false),
  ('worker', 'intake_auto_promote_min_confidence', '{"value": 60}'::jsonb,
   'Minimum candidate confidence_0_100 for auto-promote.', false),
  ('worker', 'intake_auto_promote_playbooks',
   '{"value": "elite_long,elite_short,ten_x_alert,institutional_accumulation,institutional_exit"}'::jsonb,
   'Comma-separated playbook_id list allowed for auto-promote.', false),
  ('worker', 'intake_auto_promote_max_active', '{"value": 20}'::jsonb,
   'Max engine_symbols in promoted|analyzing|ready|trading|closing for auto-promote cap.', false),
  ('worker', 'intake_auto_promote_default_interval', '{"value": "15m"}'::jsonb,
   'Default bar interval for promoted engine_symbols.', false),
  ('worker', 'lifecycle_manager_enabled', '{"enabled": false}'::jsonb,
   'Enable lifecycle_manager_loop (state transitions for intake-managed symbols).', false),
  ('worker', 'lifecycle_manager_tick_secs', '{"secs": 300}'::jsonb,
   'Tick interval seconds for lifecycle_manager_loop.', false),
  ('worker', 'lifecycle_cooldown_hours', '{"value": 24}'::jsonb,
   'Hours in cooldown before transition to retired.', false),
  ('worker', 'lifecycle_retire_stale_hours', '{"value": 48}'::jsonb,
   'Stale hours before force-retire non-manual lifecycle rows with no position.', false)
ON CONFLICT (module, config_key) DO NOTHING;


INSERT INTO app_config (key, value, description)
VALUES (
  'kill_switch_trading_halted',
  'false'::jsonb,
  'Trading halt — false: yeni emirlere izin (worker/API senkronu)'
)
ON CONFLICT (key) DO UPDATE SET
  value = EXCLUDED.value,
  updated_at = now();