-- 0160_faz10_candle_stage3.sql
--
-- Faz 10 Aşama 3 — Japanese candlestick detector config seeds.
-- CLAUDE.md #2: tüm eşikler system_config üzerinden, hard-code yok.
-- Idempotent: ON CONFLICT DO NOTHING.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('detection', 'candle.enabled', 'true'::jsonb,
   'Candlestick detector family toggle.'),
  ('detection', 'candle.doji_body_ratio_max', '0.1'::jsonb,
   'Doji için max body/range oranı.'),
  ('detection', 'candle.marubozu_shadow_ratio_max', '0.05'::jsonb,
   'Marubozu için max (toplam gölge)/range oranı.'),
  ('detection', 'candle.hammer_lower_shadow_ratio_min', '2.0'::jsonb,
   'Hammer için min lower_shadow/body oranı.'),
  ('detection', 'candle.hammer_upper_shadow_ratio_max', '0.5'::jsonb,
   'Hammer için max upper_shadow/body oranı.'),
  ('detection', 'candle.spinning_top_body_ratio_max', '0.3'::jsonb,
   'Spinning top için max body/range oranı.'),
  ('detection', 'candle.tweezer_price_tol', '0.002'::jsonb,
   'Tweezer için high/low eşitlik toleransı (fraksiyon).'),
  ('detection', 'candle.trend_context_bars', '5'::jsonb,
   'Reversal onayı için ön trend uzunluğu (bar).'),
  ('detection', 'candle.trend_context_min_pct', '0.015'::jsonb,
   'Ön trend min kümülatif getirisi |ret|.'),
  ('detection', 'candle.min_structural_score', '0.5'::jsonb,
   'Candle detection için min structural score.')
ON CONFLICT (module, config_key) DO NOTHING;
