-- 0024_qtss_v2_tbm_seed.sql
--
-- Faz 7.5 Adım 2 — TBM (Top/Bottom Mining) reversal detector config seed.
--
-- The v2 TBM detector hydrates `TbmConfig` from system_config on every
-- tick. CLAUDE.md #2: every tunable lives in the table — no defaults
-- baked into worker code paths. CLAUDE.md #1: each pillar weight is its
-- own row so the operator can rebalance without touching code.

-- Master switch + cadence ----------------------------------------------
SELECT _qtss_register_key('tbm.enabled', 'tbm','runtime','bool',
    'false'::jsonb, NULL,
    'Master switch for the v2 TBM reversal detector loop.',
    'toggle', true, 'normal', ARRAY['tbm','runtime']);

SELECT _qtss_register_key('tbm.tick_interval_s', 'tbm','runtime','int',
    '60'::jsonb, 'seconds',
    'How often the TBM detector loop scans active engine_symbols.',
    'number', true, 'normal', ARRAY['tbm','runtime']);

SELECT _qtss_register_key('tbm.lookback_bars', 'tbm','runtime','int',
    '300'::jsonb, 'bars',
    'How many recent bars the TBM detector pulls per (symbol, timeframe) tick.',
    'number', true, 'normal', ARRAY['tbm','runtime']);

SELECT _qtss_register_key('tbm.onchain_enabled', 'tbm','runtime','bool',
    'false'::jsonb, NULL,
    'Whether the onchain pillar is fed into the TBM scorer. Disabled until Faz 7.7 onchain rewrite lands.',
    'toggle', true, 'normal', ARRAY['tbm','runtime']);

-- Pillar weights -------------------------------------------------------
SELECT _qtss_register_key('tbm.pillar.momentum.weight', 'tbm','pillar','float',
    '0.30'::jsonb, NULL,
    'Weight of the Momentum pillar in the aggregate TBM score (0..1).',
    'number', true, 'normal', ARRAY['tbm','pillar']);

SELECT _qtss_register_key('tbm.pillar.volume.weight', 'tbm','pillar','float',
    '0.25'::jsonb, NULL,
    'Weight of the Volume pillar in the aggregate TBM score (0..1).',
    'number', true, 'normal', ARRAY['tbm','pillar']);

SELECT _qtss_register_key('tbm.pillar.structure.weight', 'tbm','pillar','float',
    '0.30'::jsonb, NULL,
    'Weight of the Structure pillar (Fib/BB/formations) in the aggregate TBM score (0..1).',
    'number', true, 'normal', ARRAY['tbm','pillar']);

SELECT _qtss_register_key('tbm.pillar.onchain.weight', 'tbm','pillar','float',
    '0.15'::jsonb, NULL,
    'Weight of the Onchain pillar when onchain data is available.',
    'number', true, 'normal', ARRAY['tbm','pillar']);

-- Setup detection thresholds -------------------------------------------
SELECT _qtss_register_key('tbm.setup.min_score', 'tbm','setup','float',
    '50.0'::jsonb, NULL,
    'Minimum aggregate TBM score required to emit a setup (0..100).',
    'number', true, 'normal', ARRAY['tbm','setup']);

SELECT _qtss_register_key('tbm.setup.min_active_pillars', 'tbm','setup','int',
    '2'::jsonb, NULL,
    'Minimum number of pillars whose individual score crosses pillar_active_threshold for the setup to count.',
    'number', true, 'normal', ARRAY['tbm','setup']);

SELECT _qtss_register_key('tbm.setup.pillar_active_threshold', 'tbm','setup','float',
    '20.0'::jsonb, NULL,
    'Per-pillar score above which a pillar counts as active for the active-pillars rule (0..100).',
    'number', false, 'normal', ARRAY['tbm','setup']);

-- Multi-timeframe confirmation -----------------------------------------
SELECT _qtss_register_key('tbm.mtf.required_confirms', 'tbm','mtf','int',
    '2'::jsonb, NULL,
    'Minimum count of confirming sibling timeframes before a TBM setup is promoted to confirmed.',
    'number', true, 'normal', ARRAY['tbm','mtf']);

SELECT _qtss_register_key('tbm.mtf.min_alignment', 'tbm','mtf','float',
    '0.5'::jsonb, NULL,
    'Minimum cross-timeframe alignment score (0..1) required for MTF confirmation.',
    'number', false, 'normal', ARRAY['tbm','mtf']);

-- Validator boost ------------------------------------------------------
-- The v2 validator pulls the latest TBM score for the (symbol, tf) and
-- nudges confidence on existing reversal patterns when TBM agrees.
SELECT _qtss_register_key('validator.tbm_boost.enabled', 'validator','tbm_boost','bool',
    'false'::jsonb, NULL,
    'Whether the validator should inject TBM confluence into pattern confidence.',
    'toggle', true, 'normal', ARRAY['validator','tbm_boost']);

SELECT _qtss_register_key('validator.tbm_boost.max_delta', 'validator','tbm_boost','float',
    '0.15'::jsonb, NULL,
    'Maximum confidence boost (additive, 0..1) the TBM pillar can apply to a pattern.',
    'number', false, 'normal', ARRAY['validator','tbm_boost']);

-- ---------------------------------------------------------------------
-- Seed runtime values into system_config (the table the worker reads).
--
-- `_qtss_register_key` only populates `config_schema` (the schema/UI
-- catalog). The worker resolves values from `system_config`, so a fresh
-- install needs an explicit row per key — otherwise UPDATE statements
-- from operators silently match zero rows. ON CONFLICT keeps repeated
-- migrations idempotent without overwriting operator-tuned values.
-- ---------------------------------------------------------------------

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('tbm', 'enabled',                       'false'::jsonb, 'Master switch for the v2 TBM reversal detector loop.'),
    ('tbm', 'tick_interval_s',               '60'::jsonb,    'How often the TBM detector loop scans active engine_symbols.'),
    ('tbm', 'lookback_bars',                 '300'::jsonb,   'How many recent bars the TBM detector pulls per (symbol, timeframe) tick.'),
    ('tbm', 'onchain_enabled',               'false'::jsonb, 'Whether the onchain pillar is fed into the TBM scorer.'),
    ('tbm', 'pillar.momentum.weight',        '0.30'::jsonb,  'Weight of the Momentum pillar (0..1).'),
    ('tbm', 'pillar.volume.weight',          '0.25'::jsonb,  'Weight of the Volume pillar (0..1).'),
    ('tbm', 'pillar.structure.weight',       '0.30'::jsonb,  'Weight of the Structure pillar (0..1).'),
    ('tbm', 'pillar.onchain.weight',         '0.15'::jsonb,  'Weight of the Onchain pillar (0..1).'),
    ('tbm', 'setup.min_score',               '50.0'::jsonb,  'Minimum aggregate TBM score required to emit a setup (0..100).'),
    ('tbm', 'setup.min_active_pillars',      '2'::jsonb,     'Minimum number of active pillars for the setup to count.'),
    ('tbm', 'setup.pillar_active_threshold', '20.0'::jsonb,  'Per-pillar score above which a pillar counts as active (0..100).'),
    ('tbm', 'mtf.required_confirms',         '2'::jsonb,     'Minimum count of confirming sibling timeframes for MTF.'),
    ('tbm', 'mtf.min_alignment',             '0.5'::jsonb,   'Minimum cross-timeframe alignment score (0..1).'),
    ('validator', 'tbm_boost.enabled',       'false'::jsonb, 'Whether the validator should inject TBM confluence into pattern confidence.'),
    ('validator', 'tbm_boost.max_delta',     '0.15'::jsonb,  'Maximum confidence boost (additive, 0..1) the TBM pillar can apply.')
ON CONFLICT (module, config_key) DO NOTHING;
