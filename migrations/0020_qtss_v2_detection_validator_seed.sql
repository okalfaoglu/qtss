-- 0020_qtss_v2_detection_validator_seed.sql
--
-- Faz 7 Adım 3 — Detection validator config seed.
--
-- The validator loop in qtss-worker reads forming detections out of
-- qtss_v2_detections, runs the qtss-validator confirmation channels,
-- and writes back confidence + channel_scores. CLAUDE.md #2: every
-- toggle/threshold lives in system_config, never in source.

SELECT _qtss_register_key('detection.validator.enabled', 'detection','validator','bool',
    'false'::jsonb, NULL,
    'Master switch for the v2 detection validator loop in qtss-worker. Off until rollout.',
    'toggle', true, 'normal', ARRAY['detection','validator']);

SELECT _qtss_register_key('detection.validator.tick_interval_s', 'detection','validator','int',
    '5'::jsonb, 'seconds',
    'How often the validator polls qtss_v2_detections for unscored forming rows.',
    'number', true, 'normal', ARRAY['detection','validator']);

SELECT _qtss_register_key('detection.validator.batch_limit', 'detection','validator','int',
    '50'::jsonb, 'rows',
    'Maximum number of unscored detections drained per validator tick.',
    'number', false, 'normal', ARRAY['detection','validator']);

SELECT _qtss_register_key('detection.validator.min_confidence', 'detection','validator','float',
    '0.55'::jsonb, NULL,
    'Final blended confidence floor; below this the detection is marked invalidated.',
    'number', true, 'normal', ARRAY['detection','validator']);

SELECT _qtss_register_key('detection.validator.structural_weight', 'detection','validator','float',
    '0.5'::jsonb, NULL,
    'Weight given to detector structural_score in the blended confidence (0..1).',
    'number', false, 'normal', ARRAY['detection','validator']);

SELECT _qtss_register_key('detection.validator.hit_rate_min_samples', 'detection','validator','int',
    '20'::jsonb, 'samples',
    'Minimum historical samples before the historical_hit_rate channel will speak.',
    'number', false, 'normal', ARRAY['detection','validator']);
