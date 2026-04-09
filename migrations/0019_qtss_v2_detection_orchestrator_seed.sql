-- 0019_qtss_v2_detection_orchestrator_seed.sql
--
-- Faz 7 Adım 1 — Detection orchestrator config seed.
--
-- The orchestrator (Adım 2) reads which detector families are active
-- and how many recent bars to feed each invocation from system_config.
-- CLAUDE.md #2: never hardcode such toggles in source.
--
-- These keys complement the per-family confidence floors already
-- registered in 0016_qtss_v2_config_seed.sql.
--
-- Defensive re-create of the helper function: some pre-Faz-7 deployments
-- recorded the original (10-arg) version of `_qtss_register_key` without
-- the `p_tags` parameter, so calls below fail with "function does not
-- exist". CREATE OR REPLACE is idempotent on the matching signature.

CREATE OR REPLACE FUNCTION _qtss_register_key(
    p_key             TEXT,
    p_category        TEXT,
    p_subcategory     TEXT,
    p_value_type      TEXT,
    p_default         JSONB,
    p_unit            TEXT,
    p_description     TEXT,
    p_ui_widget       TEXT,
    p_requires_restart BOOLEAN,
    p_sensitivity     TEXT,
    p_tags            TEXT[]
) RETURNS VOID AS $$
BEGIN
    INSERT INTO config_schema (
        key, category, subcategory, value_type, default_value,
        unit, description, ui_widget, requires_restart, sensitivity,
        introduced_in, tags
    ) VALUES (
        p_key, p_category, p_subcategory, p_value_type, p_default,
        p_unit, p_description, p_ui_widget, p_requires_restart, p_sensitivity,
        '0019', p_tags
    )
    ON CONFLICT (key) DO NOTHING;
END;
$$ LANGUAGE plpgsql;

SELECT _qtss_register_key('detection.orchestrator.enabled', 'detection','orchestrator','bool',
    'false'::jsonb, NULL,
    'Master switch for the v2 detector orchestrator loop in qtss-worker. Off by default until rollout.',
    'toggle', true, 'normal', ARRAY['detection','orchestrator']);

SELECT _qtss_register_key('detection.orchestrator.tick_interval_s', 'detection','orchestrator','int',
    '5'::jsonb, 'seconds',
    'How often the orchestrator polls each (symbol, timeframe) for new bars.',
    'number', true, 'normal', ARRAY['detection','orchestrator']);

SELECT _qtss_register_key('detection.orchestrator.history_bars', 'detection','orchestrator','int',
    '500'::jsonb, 'bars',
    'Number of recent bars fed to the pivot engine on each orchestrator tick.',
    'number', false, 'normal', ARRAY['detection','orchestrator']);

SELECT _qtss_register_key('detection.elliott.enabled',   'detection','elliott',  'bool',
    'true'::jsonb, NULL, 'Enable the Elliott impulse detector.',
    'toggle', true, 'normal', ARRAY['detection','elliott']);

SELECT _qtss_register_key('detection.harmonic.enabled',  'detection','harmonic', 'bool',
    'true'::jsonb, NULL, 'Enable the harmonic XABCD detector.',
    'toggle', true, 'normal', ARRAY['detection','harmonic']);

SELECT _qtss_register_key('detection.classical.enabled', 'detection','classical','bool',
    'true'::jsonb, NULL, 'Enable the classical chart pattern detector.',
    'toggle', true, 'normal', ARRAY['detection','classical']);

SELECT _qtss_register_key('detection.wyckoff.enabled',   'detection','wyckoff',  'bool',
    'true'::jsonb, NULL, 'Enable the Wyckoff phase detector.',
    'toggle', true, 'normal', ARRAY['detection','wyckoff']);

-- The chart endpoint pulls the latest N detections per (symbol,tf) for
-- the overlay. Tunable so the GUI can shrink it under load.
SELECT _qtss_register_key('detection.chart_overlay.limit', 'detection','chart','int',
    '50'::jsonb, 'rows',
    'Maximum number of detections returned per (symbol,timeframe) by /v2/chart overlays.',
    'number', false, 'normal', ARRAY['detection','chart']);
