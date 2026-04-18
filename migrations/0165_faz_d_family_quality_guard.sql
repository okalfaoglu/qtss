-- 0165_faz_d_family_quality_guard.sql
--
-- Faz D — Family quality guard channel seeds.
--
-- The FamilyQualityGuard validator channel enforces per-family
-- structural sanity rules (min anchor count, Elliott wave alternation)
-- as a final safety net after the detector emits. Every threshold is
-- DB-tunable via Config Editor (CLAUDE.md #2).
--
-- Idempotent: ON CONFLICT DO NOTHING.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('detection', 'validator.family_guard.harmonic_min_anchors', '5'::jsonb,
   'Harmonic XABCD için min anchor sayısı (gartley/butterfly/bat/crab/cypher/shark → 5).'),
  ('detection', 'validator.family_guard.elliott_impulse_min_anchors', '6'::jsonb,
   'Elliott impulse/diagonal/triangle için min anchor sayısı (5 dalga = 6 pivot).'),
  ('detection', 'validator.family_guard.elliott_correction_min_anchors', '4'::jsonb,
   'Elliott zigzag/flat correction için min anchor sayısı (0,A,B,C → 4).'),
  ('detection', 'validator.family_guard.classical_min_anchors', '3'::jsonb,
   'Classical patterns için min anchor sayısı (double_top/bottom/v_spike → 3).'),
  ('detection', 'validator.family_guard.candle_min_anchors', '2'::jsonb,
   'Candlestick patterns için min anchor sayısı (open + close → 2).'),
  ('detection', 'validator.family_guard.gap_min_anchors', '2'::jsonb,
   'Gap patterns için min anchor sayısı (pre + post gap → 2).'),
  ('detection', 'validator.family_guard.elliott_alternation_enabled', '1.0'::jsonb,
   'Elliott impulse için wave-2<wave-1 ve wave-4<wave-3 alternation kuralı (1=açık, 0=kapalı).'),
  ('detection', 'validator.family_guard.weight', '2.0'::jsonb,
   'FamilyQualityGuard kanalının blend içindeki ağırlığı. İhlal (score=0) durumunda confidence düşüşünün ağırlığını belirler.')
ON CONFLICT (module, config_key) DO NOTHING;
