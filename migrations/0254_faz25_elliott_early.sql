-- FAZ 25 PR-25A — Elliott early-wave detection.
--
-- The base ElliottWriter persists complete 6-pivot motives, post-motive
-- ABCs and triangles. Those are LATE signals (by the time a 5-wave
-- impulse closes the Wave 3 ride is already over). PR-25A introduces
-- nascent / forming / extended impulse detection on the same pivot
-- tape and writes them under `pattern_family = 'elliott_early'`.
--
-- Strictly additive (FAZ 25 §0 isolation principle):
--   * Existing T and D allocator profiles are untouched
--   * Existing `motive`, `abc`, `triangle` rows in `detections` keep
--     flowing exactly as before
--   * Confluence scorer doesn't read 'elliott_early' yet — IQ-D
--     candidate creator (PR-25C) will be the first consumer
--
-- This migration only seeds config + adds an explanatory scorer
-- weight (initialized at 0.0 so we don't accidentally pollute existing
-- D/T setup confidence calculations until IQ-D is wired up).

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('elliott_early', 'enabled',
     '{"enabled": true}'::jsonb,
     'Master switch for early-wave Elliott detection (nascent + forming + extended). Persisted under pattern_family=''elliott_early''. Disable to silence the writer without rebuilding.'),

    ('elliott_early', 'min_score',
     '{"value": 0.30}'::jsonb,
     'Minimum fib-proximity score (0..1) for an early-wave detection to be persisted. Below this the pattern is too far from canonical Elliott ratios. Default 0.30 = roughly 30%-fib-snap quality.'),

    ('confluence', 'weights.elliott_early',
     '{"value": 0.0}'::jsonb,
     'Confluence weight for elliott_early detections. Initialized at 0.0 because the scorer is shared with existing D/T setups; raising this above zero would bleed early-wave influence into legacy setups before the IQ-D pipeline is ready (FAZ 25 PR-25C+). Will be set to 1.3 once IQ-D candidate creator goes live.')
ON CONFLICT (module, config_key) DO NOTHING;
