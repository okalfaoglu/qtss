-- 0164_faz_a_candle_tf_gate.sql
--
-- Faz A — Candle timeframe gate.
--
-- Candlestick formations emit too much noise on sub-15m timeframes
-- (e.g. morning_star on 1m has near-random hit rate). Add a single
-- DB-tunable threshold; detectors skip below it.
--
-- CLAUDE.md #2: GUI-editable via Config Editor, no hard-code.
-- Idempotent: ON CONFLICT DO NOTHING.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('detection', 'candle.min_timeframe_seconds', '900'::jsonb,
   'Candlestick patterns için minimum timeframe (saniye). Altındaki TF''lerde candle detection emit edilmez. Default 900 = 15m.')
ON CONFLICT (module, config_key) DO NOTHING;
