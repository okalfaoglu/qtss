-- 0030_qtss_v2_detection_orchestrator_enable.sql
--
-- Faz 7.7b / A — make sure the v2 detection orchestrator and every
-- per-family toggle it gates are turned ON in `system_config`. The
-- orchestrator loop is gated by `detection.orchestrator.enabled` and
-- defaults to `false` in code (`resolve_worker_enabled_flag`), so a
-- fresh DB silently produces zero detections until somebody flips it.
--
-- This migration only inserts rows that are missing — it never
-- overwrites an operator decision (`ON CONFLICT DO NOTHING`).

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection', 'orchestrator.enabled',     'true'::jsonb, 'Run the v2 detection orchestrator loop.'),
    ('detection', 'orchestrator.tick_interval_s', '5'::jsonb, 'Orchestrator pass interval (s).'),
    ('detection', 'elliott.enabled',          'true'::jsonb, 'Enable Elliott detector family.'),
    ('detection', 'harmonic.enabled',         'true'::jsonb, 'Enable Harmonic detector family.'),
    ('detection', 'classical.enabled',        'true'::jsonb, 'Enable Classical detector family.'),
    ('detection', 'wyckoff.enabled',          'true'::jsonb, 'Enable Wyckoff detector family.'),
    ('detection', 'range.enabled',            'true'::jsonb, 'Enable trading-range detector family (Faz 7.7b — RangeRunner bridges qtss-chart-patterns into qtss_v2_detections).')
ON CONFLICT (module, config_key) DO NOTHING;
