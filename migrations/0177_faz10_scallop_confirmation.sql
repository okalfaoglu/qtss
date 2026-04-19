-- 0177_faz10_scallop_confirmation.sql
--
-- Faz 10 Aşama 4.2 — Scallop detector breakout + volume confirmation.
--
-- Why: Tightened thresholds in 0176 helped, but the detector still
-- fired on the pivot itself without requiring price to actually break
-- out of the J-shape. Adds two gates after RimR:
--   1. At least one bar within `confirm_lookback` bars closes beyond
--      RimR by `breakout_atr_mult * ATR` (minimum price follow-through).
--   2. That bar's volume ≥ `breakout_vol_mult * avg(prev N bars)`
--      (demand/supply confirmation).
-- Score formula also rebalanced to include breakout + volume sub-scores.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('detection', 'classical.scallop_confirm_lookback', '5'::jsonb,
   'Scallop: RimR sonrası kaç bar içinde breakout aranır.'),
  ('detection', 'classical.scallop_breakout_atr_mult', '0.25'::jsonb,
   'Scallop breakout eşiği: close RimR''den en az bu * ATR uzaklıkta olmalı.'),
  ('detection', 'classical.scallop_breakout_vol_mult', '1.3'::jsonb,
   'Scallop breakout volume çarpanı: breakout bar hacmi avg''nin en az bu katı.'),
  ('detection', 'classical.scallop_vol_avg_window', '20'::jsonb,
   'Scallop volume trailing average penceresi (bar).'),
  ('detection', 'classical.scallop_atr_period', '14'::jsonb,
   'Scallop breakout ATR periyodu (bar).')
ON CONFLICT (module, config_key) DO NOTHING;
