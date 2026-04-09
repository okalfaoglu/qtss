-- 0026_qtss_v2_onchain_metrics.sql
--
-- Faz 7.7 / B3 — storage for the new (Hat A replacement) onchain
-- pipeline. The legacy `onchain_signal_scores` table stays untouched
-- for one more release so the soft-disabled Hat A can be re-enabled
-- in a hurry; the v2 worker writes here instead.
--
-- One row per (symbol, fetcher tick). We do *not* upsert by symbol —
-- the worker keeps a short history so the GUI can chart the score
-- over time, and an index on (symbol, computed_at DESC) keeps the
-- "latest per symbol" lookup cheap.

CREATE TABLE IF NOT EXISTS qtss_v2_onchain_metrics (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol             TEXT        NOT NULL,
    computed_at        TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- Per-category readings in [-1, +1] (NULL when fetcher disabled
    -- or returned an error). Kept as separate columns instead of one
    -- JSONB blob so SQL filters / GUI panels can index them cleanly.
    derivatives_score  DOUBLE PRECISION,
    stablecoin_score   DOUBLE PRECISION,
    chain_score        DOUBLE PRECISION,

    -- Aggregated outputs consumed by the TBM Onchain pillar.
    aggregate_score    DOUBLE PRECISION NOT NULL,  -- 0..1
    direction          TEXT             NOT NULL,  -- long|short|neutral
    confidence         DOUBLE PRECISION NOT NULL,  -- 0..1

    -- Free-form bag: per-fetcher details list, raw inputs, error
    -- fingerprints. Worker fills this best-effort for ops debugging.
    raw_meta           JSONB            NOT NULL DEFAULT '{}'::jsonb,

    CONSTRAINT qtss_v2_onchain_metrics_direction_chk
        CHECK (direction IN ('long','short','neutral'))
);

CREATE INDEX IF NOT EXISTS qtss_v2_onchain_metrics_symbol_time_idx
    ON qtss_v2_onchain_metrics (symbol, computed_at DESC);

-- ---------------------------------------------------------------------
-- system_config seeds (CLAUDE.md #2 — every tunable in the table).
-- ---------------------------------------------------------------------

SELECT _qtss_register_key('onchain.enabled', 'onchain','runtime','bool',
    'false'::jsonb, NULL,
    'Master switch for the v2 onchain fetcher loop (Faz 7.7).',
    'toggle', true, 'normal', ARRAY['onchain','runtime']);

SELECT _qtss_register_key('onchain.tick_interval_s', 'onchain','runtime','int',
    '300'::jsonb, 'seconds',
    'How often the v2 onchain loop refreshes per-symbol category readings.',
    'number', true, 'normal', ARRAY['onchain','runtime']);

SELECT _qtss_register_key('onchain.stale_after_s', 'onchain','runtime','int',
    '1800'::jsonb, 'seconds',
    'Maximum age the TBM bridge accepts before treating a row as missing.',
    'number', true, 'normal', ARRAY['onchain','runtime']);

SELECT _qtss_register_key('onchain.fetcher.derivatives.enabled', 'onchain','fetcher','bool',
    'true'::jsonb, NULL,
    'Enable Binance public derivatives fetcher (free, every symbol).',
    'toggle', true, 'normal', ARRAY['onchain','fetcher']);

SELECT _qtss_register_key('onchain.fetcher.stablecoin.enabled', 'onchain','fetcher','bool',
    'true'::jsonb, NULL,
    'Enable DeFiLlama + alternative.me macro fetcher (free, market-wide).',
    'toggle', true, 'normal', ARRAY['onchain','fetcher']);

SELECT _qtss_register_key('onchain.fetcher.glassnode.enabled', 'onchain','fetcher','bool',
    'false'::jsonb, NULL,
    'Enable Glassnode cohort fetcher (paid, BTC/ETH only — needs api key).',
    'toggle', true, 'normal', ARRAY['onchain','fetcher']);

SELECT _qtss_register_key('onchain.fetcher.glassnode.api_key', 'onchain','fetcher','string',
    '""'::jsonb, NULL,
    'Glassnode API key. Empty disables the fetcher even when its enabled flag is true.',
    'text', true, 'high', ARRAY['onchain','fetcher','secret']);

SELECT _qtss_register_key('onchain.aggregator.weight.derivatives', 'onchain','aggregator','float',
    '0.5'::jsonb, NULL,
    'Aggregate weight assigned to the derivatives category (0..1).',
    'number', true, 'normal', ARRAY['onchain','aggregator']);

SELECT _qtss_register_key('onchain.aggregator.weight.stablecoin', 'onchain','aggregator','float',
    '0.3'::jsonb, NULL,
    'Aggregate weight assigned to the stablecoin/macro category (0..1).',
    'number', true, 'normal', ARRAY['onchain','aggregator']);

SELECT _qtss_register_key('onchain.aggregator.weight.chain', 'onchain','aggregator','float',
    '0.2'::jsonb, NULL,
    'Aggregate weight assigned to the on-chain (Glassnode) category (0..1).',
    'number', true, 'normal', ARRAY['onchain','aggregator']);

-- Hat A retirement flag: read by the legacy onchain_signal_scorer to
-- short-circuit itself. Defaults to true so a fresh deploy ships with
-- Hat A off (Faz 7.7 cutover policy). Operator can flip it back if
-- the v2 fetcher loop misbehaves.
SELECT _qtss_register_key('onchain.legacy_v1.disabled', 'onchain','runtime','bool',
    'true'::jsonb, NULL,
    'Soft-disable for the legacy Hat A onchain_signal_scorer pipeline.',
    'toggle', true, 'normal', ARRAY['onchain','runtime']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('onchain', 'enabled',                          'false'::jsonb, 'Master switch for the v2 onchain fetcher loop.'),
    ('onchain', 'tick_interval_s',                  '300'::jsonb,   'How often the v2 onchain loop refreshes per-symbol readings.'),
    ('onchain', 'stale_after_s',                    '1800'::jsonb,  'Maximum age the TBM bridge accepts before treating a row as missing.'),
    ('onchain', 'fetcher.derivatives.enabled',      'true'::jsonb,  'Enable Binance public derivatives fetcher.'),
    ('onchain', 'fetcher.stablecoin.enabled',       'true'::jsonb,  'Enable DeFiLlama + alternative.me macro fetcher.'),
    ('onchain', 'fetcher.glassnode.enabled',        'false'::jsonb, 'Enable Glassnode cohort fetcher.'),
    ('onchain', 'fetcher.glassnode.api_key',        '""'::jsonb,    'Glassnode API key (empty disables).'),
    ('onchain', 'aggregator.weight.derivatives',    '0.5'::jsonb,   'Derivatives category weight.'),
    ('onchain', 'aggregator.weight.stablecoin',     '0.3'::jsonb,   'Stablecoin/macro category weight.'),
    ('onchain', 'aggregator.weight.chain',          '0.2'::jsonb,   'Chain (Glassnode) category weight.'),
    ('onchain', 'legacy_v1.disabled',               'true'::jsonb,  'Soft-disable for the legacy Hat A onchain pipeline.')
ON CONFLICT (module, config_key) DO NOTHING;
