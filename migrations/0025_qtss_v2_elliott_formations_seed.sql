-- 0025_qtss_v2_elliott_formations_seed.sql
--
-- Faz 7.6 / A1 — Per-formation enable toggles for the expanded
-- Elliott detector set.
--
-- The v2 ElliottDetectorSet hydrates `ElliottFormationToggles` from
-- system_config on every orchestrator pass, so an operator can flip
-- a formation on/off without restarting the worker. CLAUDE.md #2:
-- every tunable lives in the table.
--
-- One row per formation in `config_schema` (UI catalog) AND
-- `system_config` (the table the worker actually reads). Migration
-- 0024 documented why both are needed: `_qtss_register_key` only
-- writes to config_schema, so operator UPDATE statements would
-- silently match zero rows on a fresh install without an explicit
-- INSERT into system_config.

-- Schema catalog ------------------------------------------------------
SELECT _qtss_register_key('detection.elliott.impulse.enabled',
    'detection','elliott','bool', 'true'::jsonb, NULL,
    'Enable the canonical 5-wave Impulse detector.',
    'toggle', true, 'normal', ARRAY['detection','elliott']);

SELECT _qtss_register_key('detection.elliott.leading_diagonal.enabled',
    'detection','elliott','bool', 'false'::jsonb, NULL,
    'Enable the Leading Diagonal (5-3-5-3-5 wedge) detector.',
    'toggle', true, 'normal', ARRAY['detection','elliott']);

SELECT _qtss_register_key('detection.elliott.ending_diagonal.enabled',
    'detection','elliott','bool', 'false'::jsonb, NULL,
    'Enable the Ending Diagonal (3-3-3-3-3 wedge) detector.',
    'toggle', true, 'normal', ARRAY['detection','elliott']);

SELECT _qtss_register_key('detection.elliott.zigzag.enabled',
    'detection','elliott','bool', 'false'::jsonb, NULL,
    'Enable the Zigzag (A-B-C, 5-3-5) corrective detector.',
    'toggle', true, 'normal', ARRAY['detection','elliott']);

SELECT _qtss_register_key('detection.elliott.flat.enabled',
    'detection','elliott','bool', 'false'::jsonb, NULL,
    'Enable the Flat (A-B-C, 3-3-5) corrective detector — regular/expanded/running.',
    'toggle', true, 'normal', ARRAY['detection','elliott']);

SELECT _qtss_register_key('detection.elliott.triangle.enabled',
    'detection','elliott','bool', 'false'::jsonb, NULL,
    'Enable the Triangle (A-B-C-D-E, 3-3-3-3-3) detector — contracting/expanding/barrier.',
    'toggle', true, 'normal', ARRAY['detection','elliott']);

SELECT _qtss_register_key('detection.elliott.extended_impulse.enabled',
    'detection','elliott','bool', 'false'::jsonb, NULL,
    'Enable the Extended Impulse detector (which of w1/w3/w5 is the extended wave).',
    'toggle', true, 'normal', ARRAY['detection','elliott']);

SELECT _qtss_register_key('detection.elliott.truncated_fifth.enabled',
    'detection','elliott','bool', 'false'::jsonb, NULL,
    'Enable the Truncated 5th Impulse detector (failure 5 / momentum exhaustion).',
    'toggle', true, 'normal', ARRAY['detection','elliott']);

SELECT _qtss_register_key('detection.elliott.combination.enabled',
    'detection','elliott','bool', 'false'::jsonb, NULL,
    'Enable W-X-Y / W-X-Y-X-Z combination corrections (history-aware; stub until 7.6 follow-up).',
    'toggle', true, 'normal', ARRAY['detection','elliott']);

-- Runtime values ------------------------------------------------------
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection', 'elliott.impulse.enabled',          'true'::jsonb,  'Enable canonical 5-wave Impulse detector.'),
    ('detection', 'elliott.leading_diagonal.enabled', 'false'::jsonb, 'Enable Leading Diagonal detector.'),
    ('detection', 'elliott.ending_diagonal.enabled',  'false'::jsonb, 'Enable Ending Diagonal detector.'),
    ('detection', 'elliott.zigzag.enabled',           'false'::jsonb, 'Enable Zigzag (A-B-C) detector.'),
    ('detection', 'elliott.flat.enabled',             'false'::jsonb, 'Enable Flat (regular/expanded/running) detector.'),
    ('detection', 'elliott.triangle.enabled',         'false'::jsonb, 'Enable Triangle (contracting/expanding/barrier) detector.'),
    ('detection', 'elliott.extended_impulse.enabled', 'false'::jsonb, 'Enable Extended Impulse detector.'),
    ('detection', 'elliott.truncated_fifth.enabled',  'false'::jsonb, 'Enable Truncated 5th detector.'),
    ('detection', 'elliott.combination.enabled',      'false'::jsonb, 'Enable W-X-Y / W-X-Y-X-Z combination corrections (stub).')
ON CONFLICT (module, config_key) DO NOTHING;
