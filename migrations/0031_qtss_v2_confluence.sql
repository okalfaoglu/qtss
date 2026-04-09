-- 0031_qtss_v2_confluence.sql
--
-- Faz 7.8 — dual-track confluence scoring.
--
-- Storage table for `qtss-confluence::ConfluenceReading`. One row per
-- (exchange, symbol, timeframe, computed_at). The Setup Engine
-- (Faz 8.0) reads the latest row per (symbol, timeframe) and gates
-- on `guven >= threshold` before arming any setup.
--
-- All thresholds, weights and the min_layers rule are config-driven
-- (CLAUDE.md #2) — this migration only registers the schema and
-- bootstrap defaults.

CREATE TABLE IF NOT EXISTS qtss_v2_confluence (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    computed_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    exchange        TEXT NOT NULL,
    symbol          TEXT NOT NULL,
    timeframe       TEXT NOT NULL,
    erken_uyari     REAL NOT NULL,                  -- [-1, +1]
    guven           REAL NOT NULL,                  -- [0, 1]
    direction       TEXT NOT NULL CHECK (direction IN ('long','short','neutral')),
    layer_count     INT  NOT NULL,
    raw_meta        JSONB NOT NULL DEFAULT '{}'::jsonb,
    UNIQUE (exchange, symbol, timeframe, computed_at)
);

CREATE INDEX IF NOT EXISTS idx_v2_confluence_latest
    ON qtss_v2_confluence (exchange, symbol, timeframe, computed_at DESC);

-- ───────────────────────── config keys ─────────────────────────────────

SELECT _qtss_register_key('confluence.enabled', 'confluence','loop','bool',
    'false'::jsonb, NULL,
    'Enable the v2 confluence scoring loop. Reads TBM + detections + onchain and writes qtss_v2_confluence.',
    'toggle', true, 'normal', ARRAY['confluence']);

SELECT _qtss_register_key('confluence.tick_interval_s', 'confluence','loop','int',
    '30'::jsonb, NULL,
    'How often the confluence loop runs (seconds).',
    'number', true, 'normal', ARRAY['confluence']);

SELECT _qtss_register_key('confluence.window_s', 'confluence','loop','int',
    '300'::jsonb, NULL,
    'Lookback window (seconds) for considering TBM/detection/onchain rows fresh enough to feed the scorer.',
    'number', true, 'normal', ARRAY['confluence']);

SELECT _qtss_register_key('confluence.weight.elliott', 'confluence','weight','float',
    '0.30'::jsonb, NULL, 'Confluence weight for Elliott family votes.',
    'number', true, 'normal', ARRAY['confluence','weight']);
SELECT _qtss_register_key('confluence.weight.harmonic', 'confluence','weight','float',
    '0.20'::jsonb, NULL, 'Confluence weight for Harmonic family votes.',
    'number', true, 'normal', ARRAY['confluence','weight']);
SELECT _qtss_register_key('confluence.weight.classical', 'confluence','weight','float',
    '0.15'::jsonb, NULL, 'Confluence weight for Classical family votes.',
    'number', true, 'normal', ARRAY['confluence','weight']);
SELECT _qtss_register_key('confluence.weight.wyckoff', 'confluence','weight','float',
    '0.15'::jsonb, NULL, 'Confluence weight for Wyckoff family votes.',
    'number', true, 'normal', ARRAY['confluence','weight']);
SELECT _qtss_register_key('confluence.weight.range', 'confluence','weight','float',
    '0.10'::jsonb, NULL, 'Confluence weight for Range family votes.',
    'number', true, 'normal', ARRAY['confluence','weight']);
SELECT _qtss_register_key('confluence.weight.tbm', 'confluence','weight','float',
    '0.10'::jsonb, NULL, 'Confluence weight for the TBM aggregate score.',
    'number', true, 'normal', ARRAY['confluence','weight']);
SELECT _qtss_register_key('confluence.weight.onchain', 'confluence','weight','float',
    '0.10'::jsonb, NULL, 'Confluence weight for the Onchain aggregate score.',
    'number', true, 'normal', ARRAY['confluence','weight']);

SELECT _qtss_register_key('confluence.min_layers', 'confluence','rule','int',
    '3'::jsonb, NULL,
    'Minimum number of distinct voting layers required for guven > 0. Below this guven is hard-zero (direction is preserved).',
    'number', true, 'normal', ARRAY['confluence','rule']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('confluence', 'enabled',           'false'::jsonb, 'Enable confluence loop.'),
    ('confluence', 'tick_interval_s',   '30'::jsonb,    'Loop tick (s).'),
    ('confluence', 'window_s',          '300'::jsonb,   'Input freshness window (s).'),
    ('confluence', 'weight.elliott',    '0.30'::jsonb,  'Elliott weight.'),
    ('confluence', 'weight.harmonic',   '0.20'::jsonb,  'Harmonic weight.'),
    ('confluence', 'weight.classical',  '0.15'::jsonb,  'Classical weight.'),
    ('confluence', 'weight.wyckoff',    '0.15'::jsonb,  'Wyckoff weight.'),
    ('confluence', 'weight.range',      '0.10'::jsonb,  'Range weight.'),
    ('confluence', 'weight.tbm',        '0.10'::jsonb,  'TBM weight.'),
    ('confluence', 'weight.onchain',    '0.10'::jsonb,  'Onchain weight.'),
    ('confluence', 'min_layers',        '3'::jsonb,     'Hard floor for guven layer count.')
ON CONFLICT (module, config_key) DO NOTHING;
