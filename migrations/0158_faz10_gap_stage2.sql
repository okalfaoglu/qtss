-- 0158_faz10_gap_stage2.sql
--
-- Faz 10 Aşama 2 — Gap & island reversal detector config seeds.
-- CLAUDE.md #2: tüm eşikler system_config üzerinden, hard-code yok.
-- Idempotent: ON CONFLICT DO NOTHING ile çift çalıştırmaya güvenli.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('detection', 'gap.enabled', 'true'::jsonb,
   'Gap/island reversal detector family toggle.'),
  ('detection', 'gap.min_gap_pct', '0.005'::jsonb,
   'Minimum |gap| olarak open/prev_close fraksiyonu.'),
  ('detection', 'gap.volume_sma_bars', '20'::jsonb,
   'Hacim SMA baz pencere (bar).'),
  ('detection', 'gap.vol_mult_breakaway', '1.5'::jsonb,
   'Breakaway gap için min hacim/SMA çarpanı.'),
  ('detection', 'gap.vol_mult_runaway', '1.3'::jsonb,
   'Runaway gap için min hacim/SMA çarpanı.'),
  ('detection', 'gap.vol_mult_exhaustion', '1.8'::jsonb,
   'Exhaustion gap için min hacim/SMA çarpanı.'),
  ('detection', 'gap.range_flat_pct', '0.02'::jsonb,
   'Breakaway öncesi konsolidasyon için max (high-low)/mid oranı.'),
  ('detection', 'gap.consolidation_lookback', '10'::jsonb,
   'Konsolidasyon ölçümü için bar penceresi.'),
  ('detection', 'gap.runaway_trend_bars', '5'::jsonb,
   'Runaway/exhaustion için ön trend uzunluğu (bar).'),
  ('detection', 'gap.runaway_trend_min_pct', '0.02'::jsonb,
   'Runaway/exhaustion için ön trend min kümülatif getirisi.'),
  ('detection', 'gap.exhaustion_reversal_bars', '5'::jsonb,
   'Exhaustion onayı için ters kesme penceresi (bar).'),
  ('detection', 'gap.island_max_bars', '10'::jsonb,
   'Island reversal için iki gap arası max bar.'),
  ('detection', 'gap.min_structural_score', '0.5'::jsonb,
   'Gap detection için min structural score.')
ON CONFLICT (module, config_key) DO NOTHING;
