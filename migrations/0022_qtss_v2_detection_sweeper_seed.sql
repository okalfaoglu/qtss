-- 0022_qtss_v2_detection_sweeper_seed.sql
--
-- Faz 7 Adım 10 — Stale forming sweeper config seed.
--
-- The sweeper loop in qtss-worker ages out qtss_v2_detections rows
-- that never made it past `forming` (validator skipped them, the
-- orchestrator re-detected with a different anchor, etc.) by flipping
-- their state to `invalidated`. CLAUDE.md #2: every knob lives in
-- system_config, never in source.

SELECT _qtss_register_key('detection.sweeper.enabled', 'detection','sweeper','bool',
    'true'::jsonb, NULL,
    'Master switch for the v2 detection sweeper loop. On by default — purely a janitor.',
    'toggle', false, 'normal', ARRAY['detection','sweeper']);

SELECT _qtss_register_key('detection.sweeper.tick_interval_s', 'detection','sweeper','int',
    '60'::jsonb, 'seconds',
    'How often the sweeper polls qtss_v2_detections for stale forming rows.',
    'number', false, 'normal', ARRAY['detection','sweeper']);

SELECT _qtss_register_key('detection.sweeper.max_age_s', 'detection','sweeper','int',
    '3600'::jsonb, 'seconds',
    'A forming detection older than this gets flipped to invalidated by the sweeper.',
    'number', true, 'normal', ARRAY['detection','sweeper']);
