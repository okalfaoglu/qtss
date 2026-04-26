-- FAZ 25.4.A — Wyckoff alignment as the 9th component of Major Dip /
-- Top composite scorer.
--
-- Theory: Elliott + Wyckoff are complementary. Elliott tells the
-- shape of the move (5-up / 3-down / which wave we're in); Wyckoff
-- tells the conviction behind it (smart money buying / selling /
-- exhaustion). Combined: shape + conviction = high-confidence
-- entry.
--
-- Canonical alignment matrix (research doc § II.1):
--   Dip + W2 + Spring        → 1.0  (Phase C accumulation, W3 launch imminent)
--   Dip + C-completed + SOW  → 0.9  (corrective exhaustion, new W1 imminent)
--   Dip + W3 + SOS           → 0.8  (Phase D, ride the markup)
--   Top + W5 + BC            → 1.0  (Phase A distribution, A-wave imminent)
--   Top + B-wave + UTAD      → 0.95 (Phase C distribution, C-wave plunge)
--   Top + W3 + SOS           → 0.0  (no top here — bullish continuation)
--
-- Weight rebalancing — old weights summed to 1.0:
--   structural_completion 0.20 + fib_retrace 0.15 + volume_capit 0.15
--   + cvd_div 0.10 + indicator 0.10 + sentiment 0.10 + multi_tf 0.10
--   + funding_oi 0.10 = 1.00
--
-- New weights — wyckoff_alignment carved from the stub channels:
--   structural_completion 0.18 + fib_retrace 0.13 + volume_capit 0.13
--   + wyckoff_alignment 0.15
--   + cvd_div 0.08 + indicator 0.08 + sentiment 0.08 + multi_tf 0.08
--   + funding_oi 0.09 = 1.00

UPDATE system_config SET value = '{"value": 0.18}'::jsonb, updated_at = now()
  WHERE module = 'major_dip' AND config_key = 'weights.structural_completion';
UPDATE system_config SET value = '{"value": 0.13}'::jsonb, updated_at = now()
  WHERE module = 'major_dip' AND config_key = 'weights.fib_retrace_quality';
UPDATE system_config SET value = '{"value": 0.13}'::jsonb, updated_at = now()
  WHERE module = 'major_dip' AND config_key = 'weights.volume_capitulation';
UPDATE system_config SET value = '{"value": 0.08}'::jsonb, updated_at = now()
  WHERE module = 'major_dip' AND config_key = 'weights.cvd_divergence';
UPDATE system_config SET value = '{"value": 0.08}'::jsonb, updated_at = now()
  WHERE module = 'major_dip' AND config_key = 'weights.indicator_alignment';
UPDATE system_config SET value = '{"value": 0.08}'::jsonb, updated_at = now()
  WHERE module = 'major_dip' AND config_key = 'weights.sentiment_extreme';
UPDATE system_config SET value = '{"value": 0.08}'::jsonb, updated_at = now()
  WHERE module = 'major_dip' AND config_key = 'weights.multi_tf_confluence';
UPDATE system_config SET value = '{"value": 0.09}'::jsonb, updated_at = now()
  WHERE module = 'major_dip' AND config_key = 'weights.funding_oi_signals';

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('major_dip', 'weights.wyckoff_alignment',
     '{"value": 0.15}'::jsonb,
     'Weight for the Wyckoff-Elliott alignment channel (FAZ 25.4.A). 0..1 score reflecting whether the latest Wyckoff event matches the Elliott wave context — Spring + W2, BC + W5, UTAD + B, SOS + W3 etc. See docs/ELLIOTT_WYCKOFF_INTEGRATION.md for the full alignment matrix.')
ON CONFLICT (module, config_key) DO UPDATE
   SET value = EXCLUDED.value,
       description = EXCLUDED.description,
       updated_at = now();
