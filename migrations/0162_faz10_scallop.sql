-- 0162_faz10_scallop.sql
--
-- Faz 10 Aşama 4 — Scallop (bullish/bearish) classical detector seeds.
-- CLAUDE.md #2: eşikler system_config üzerinden.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('detection', 'classical.scallop_min_bars', '20'::jsonb,
   'Scallop minimum süresi (rim_left → rim_right bar farkı).'),
  ('detection', 'classical.scallop_min_rim_progress_pct', '0.02'::jsonb,
   'Scallop breakout ayaklık: rim_r rim_l''den en az bu fraksiyon kadar ötede olmalı.'),
  ('detection', 'classical.scallop_roundness_r2', '0.55'::jsonb,
   'Scallop curve için parabolic R² eşiği (rounding''den biraz gevşek).')
ON CONFLICT (module, config_key) DO NOTHING;
