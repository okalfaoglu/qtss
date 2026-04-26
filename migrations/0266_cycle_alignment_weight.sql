-- FAZ 25.4.G — cycle_alignment composite component for Major Dip /
-- Major Top scoring. Reads the latest macro 4-phase cycle tile
-- (Accumulation / Markup / Distribution / Markdown) covering the
-- current bar and grades polarity alignment with source dampener
-- (confluent > elliott > event).
--
-- Other weights trimmed proportionally so the 10-component sum
-- stays at 1.00. See score_cycle_alignment in
-- crates/qtss-worker/src/major_dip_candidate_loop.rs for the
-- per-phase score matrix.

INSERT INTO system_config (module, config_key, value, description)
VALUES
  ('major_dip_candidate', 'weights.cycle_alignment',
   '{"value": 0.10}'::jsonb,
   'FAZ 25.4.G — macro 4-phase cycle context alignment weight (Accum/Markup/Dist/Markdown). Polarity-aware: Dip rewards Accumulation+Markup-early, Top rewards Distribution+Markdown-early. Source dampener: confluent=1.0, elliott=0.85, event=0.65.'),

  ('major_dip_candidate', 'weights.structural_completion',
   '{"value": 0.16}'::jsonb,
   'FAZ 25.4.G rebalance — was 0.18, trimmed to make room for cycle_alignment.'),
  ('major_dip_candidate', 'weights.fib_retrace_quality',
   '{"value": 0.12}'::jsonb,
   'FAZ 25.4.G rebalance — was 0.13.'),
  ('major_dip_candidate', 'weights.volume_capitulation',
   '{"value": 0.12}'::jsonb,
   'FAZ 25.4.G rebalance — was 0.13.'),
  ('major_dip_candidate', 'weights.cvd_divergence',
   '{"value": 0.07}'::jsonb,
   'FAZ 25.4.G rebalance — was 0.08.'),
  ('major_dip_candidate', 'weights.indicator_alignment',
   '{"value": 0.07}'::jsonb,
   'FAZ 25.4.G rebalance — was 0.08.'),
  ('major_dip_candidate', 'weights.sentiment_extreme',
   '{"value": 0.07}'::jsonb,
   'FAZ 25.4.G rebalance — was 0.08.'),
  ('major_dip_candidate', 'weights.multi_tf_confluence',
   '{"value": 0.07}'::jsonb,
   'FAZ 25.4.G rebalance — was 0.08.'),
  ('major_dip_candidate', 'weights.funding_oi_signals',
   '{"value": 0.08}'::jsonb,
   'FAZ 25.4.G rebalance — was 0.09.'),
  ('major_dip_candidate', 'weights.wyckoff_alignment',
   '{"value": 0.14}'::jsonb,
   'FAZ 25.4.G rebalance — was 0.15.')
ON CONFLICT (module, config_key)
DO UPDATE SET value = EXCLUDED.value, description = EXCLUDED.description;

-- IQ-D / IQ-T setup gate: optionally veto setups whose cycle phase
-- contradicts the polarity. Default false (advisory only) — can be
-- toggled from the config UI to enforce strict cycle-context entries.
INSERT INTO system_config (module, config_key, value, description)
VALUES
  ('iq_d_candidate', 'require_cycle_alignment',
   '{"value": false}'::jsonb,
   'FAZ 25.4.G — when true, IQ-D long candidates are vetoed if the active cycle phase is Distribution / Markdown (polarity mismatch). Default off so existing setups still flow; flip to true to enforce strict cycle-context entries.'),
  ('iq_t_candidate', 'require_cycle_alignment',
   '{"value": false}'::jsonb,
   'FAZ 25.4.G — mirror of iq_d_candidate.require_cycle_alignment for short setups. Vetoes IQ-T when cycle phase is Accumulation / Markup.')
ON CONFLICT (module, config_key)
DO UPDATE SET value = EXCLUDED.value, description = EXCLUDED.description;
