-- 0028_qtss_v2_onchain_nansen_and_discovery.sql
--
-- Faz 7.7 / D — Nansen → v2 Onchain Chain category bridge + symbol
-- discovery lifecycle.
--
-- Three things happen here:
--
-- 1. `engine_symbols` gains a lifecycle: `source` (manual /
--    top_volume / onchain_discovery), `pinned` (don't auto-prune),
--    `discovered_at`, `last_signal_at`. Existing rows are backfilled
--    as manual + pinned so nothing the operator added by hand can be
--    deleted by the pruner.
--
-- 2. Top-10 Binance USDT-perp symbols are seeded as
--    `source='top_volume', pinned=true`. The future
--    `top_volume_refresh_loop` will keep this list current; until
--    then they're a stable baseline.
--
-- 3. Config keys are registered for the Nansen fetcher (reads
--    `data_snapshots` written by the existing qtss-nansen worker
--    loops — no extra API spend), the onchain discovery loop, and
--    the pruner.
--
-- All thresholds, weights and TTLs are config-driven (CLAUDE.md #2).

-- ───────────────────────────── 1. engine_symbols lifecycle ─────────────────

ALTER TABLE engine_symbols
    ADD COLUMN IF NOT EXISTS source TEXT NOT NULL DEFAULT 'manual',
    ADD COLUMN IF NOT EXISTS pinned BOOLEAN NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS discovered_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS last_signal_at TIMESTAMPTZ;

ALTER TABLE engine_symbols
    DROP CONSTRAINT IF EXISTS engine_symbols_source_chk;
ALTER TABLE engine_symbols
    ADD CONSTRAINT engine_symbols_source_chk
    CHECK (source IN ('manual', 'top_volume', 'onchain_discovery'));

-- Backfill: anything already in the table predates the discovery
-- system → treat as manually curated and protect from pruning.
UPDATE engine_symbols
SET source = 'manual', pinned = true
WHERE source IS NULL OR source = '';

CREATE INDEX IF NOT EXISTS idx_engine_symbols_source
    ON engine_symbols (source);
CREATE INDEX IF NOT EXISTS idx_engine_symbols_discovery_ttl
    ON engine_symbols (last_signal_at)
    WHERE source = 'onchain_discovery' AND pinned = false;

-- ───────────────────────────── 2. Top-10 USDT-perp seed ────────────────────
--
-- Snapshot of top-10 Binance USDT-perp by 24h notional volume as of
-- the migration date. Seed interval is 1h; the operator (and the
-- future top_volume_refresh_loop) can add other timeframes through
-- the same insert pattern. ON CONFLICT keeps existing rows but
-- promotes them to source='top_volume' + pinned so the pruner
-- leaves them alone.

INSERT INTO engine_symbols (exchange, segment, symbol, interval, enabled, source, pinned, sort_order, label)
VALUES
    ('binance', 'futures', 'BTCUSDT',  '1h', true, 'top_volume', true,  1, 'top10'),
    ('binance', 'futures', 'ETHUSDT',  '1h', true, 'top_volume', true,  2, 'top10'),
    ('binance', 'futures', 'SOLUSDT',  '1h', true, 'top_volume', true,  3, 'top10'),
    ('binance', 'futures', 'BNBUSDT',  '1h', true, 'top_volume', true,  4, 'top10'),
    ('binance', 'futures', 'XRPUSDT',  '1h', true, 'top_volume', true,  5, 'top10'),
    ('binance', 'futures', 'DOGEUSDT', '1h', true, 'top_volume', true,  6, 'top10'),
    ('binance', 'futures', 'ADAUSDT',  '1h', true, 'top_volume', true,  7, 'top10'),
    ('binance', 'futures', 'AVAXUSDT', '1h', true, 'top_volume', true,  8, 'top10'),
    ('binance', 'futures', 'LINKUSDT', '1h', true, 'top_volume', true,  9, 'top10'),
    ('binance', 'futures', 'TRXUSDT',  '1h', true, 'top_volume', true, 10, 'top10')
ON CONFLICT (exchange, segment, symbol, interval) DO UPDATE
SET source  = 'top_volume',
    pinned  = true,
    enabled = true,
    label   = COALESCE(engine_symbols.label, EXCLUDED.label);

-- ───────────────────────── 3. Nansen fetcher config keys ───────────────────

SELECT _qtss_register_key('onchain.fetcher.nansen.enabled', 'onchain','fetcher','bool',
    'false'::jsonb, NULL,
    'Enable Nansen-derived Chain category fetcher (reads data_snapshots written by qtss-nansen — no extra API spend).',
    'toggle', true, 'normal', ARRAY['onchain','fetcher']);

SELECT _qtss_register_key('onchain.fetcher.nansen.staleness_s', 'onchain','fetcher','int',
    '7200'::jsonb, NULL,
    'Maximum age in seconds for a Nansen snapshot to count toward the score. Older = ignored.',
    'number', true, 'normal', ARRAY['onchain','fetcher']);

SELECT _qtss_register_key('onchain.fetcher.nansen.weight.netflow', 'onchain','fetcher','float',
    '0.40'::jsonb, NULL,
    'Blend weight for smart-money netflow component.',
    'number', true, 'normal', ARRAY['onchain','fetcher','weight']);

SELECT _qtss_register_key('onchain.fetcher.nansen.weight.flow_intel', 'onchain','fetcher','float',
    '0.25'::jsonb, NULL,
    'Blend weight for TGM flow-intelligence component (whale + smart money + exchange segments).',
    'number', true, 'normal', ARRAY['onchain','fetcher','weight']);

SELECT _qtss_register_key('onchain.fetcher.nansen.weight.dex_trades', 'onchain','fetcher','float',
    '0.20'::jsonb, NULL,
    'Blend weight for smart-money DEX buy/sell ratio component.',
    'number', true, 'normal', ARRAY['onchain','fetcher','weight']);

SELECT _qtss_register_key('onchain.fetcher.nansen.weight.holdings', 'onchain','fetcher','float',
    '0.15'::jsonb, NULL,
    'Blend weight for smart-money holdings 24h delta component.',
    'number', true, 'normal', ARRAY['onchain','fetcher','weight']);

SELECT _qtss_register_key('onchain.nansen.symbol_map', 'onchain','fetcher','object',
    '{}'::jsonb, NULL,
    'Symbol → Nansen token map. Shape: {"BTCUSDT":{"chain":"ethereum","address":"0x...","symbol":"WBTC"}}. Empty entries skip the symbol.',
    'json', true, 'normal', ARRAY['onchain','fetcher']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('onchain', 'fetcher.nansen.enabled',          'false'::jsonb, 'Enable Nansen Chain fetcher.'),
    ('onchain', 'fetcher.nansen.staleness_s',      '7200'::jsonb,  'Max snapshot age (s).'),
    ('onchain', 'fetcher.nansen.weight.netflow',   '0.40'::jsonb,  'Netflow component weight.'),
    ('onchain', 'fetcher.nansen.weight.flow_intel','0.25'::jsonb,  'Flow intel weight.'),
    ('onchain', 'fetcher.nansen.weight.dex_trades','0.20'::jsonb,  'DEX trades weight.'),
    ('onchain', 'fetcher.nansen.weight.holdings',  '0.15'::jsonb,  'Holdings weight.'),
    ('onchain', 'nansen.symbol_map',               '{}'::jsonb,    'Symbol → Nansen token map.')
ON CONFLICT (module, config_key) DO NOTHING;

-- ───────────────────────── 4. Discovery + pruner config keys ───────────────

SELECT _qtss_register_key('onchain.discovery.enabled', 'onchain','discovery','bool',
    'false'::jsonb, NULL,
    'Enable onchain-driven symbol discovery loop. Adds symbols with strong Nansen signals as source=onchain_discovery.',
    'toggle', true, 'normal', ARRAY['onchain','discovery']);

SELECT _qtss_register_key('onchain.discovery.tick_interval_s', 'onchain','discovery','int',
    '900'::jsonb, NULL,
    'How often the discovery scanner runs (seconds).',
    'number', true, 'normal', ARRAY['onchain','discovery']);

SELECT _qtss_register_key('onchain.discovery.min_score', 'onchain','discovery','float',
    '0.6'::jsonb, NULL,
    'Absolute aggregate score threshold for a symbol to enter discovery (|score| >= min_score).',
    'number', true, 'normal', ARRAY['onchain','discovery']);

SELECT _qtss_register_key('onchain.discovery.max_active', 'onchain','discovery','int',
    '30'::jsonb, NULL,
    'Maximum concurrent source=onchain_discovery symbols. Weakest are dropped when over cap.',
    'number', true, 'normal', ARRAY['onchain','discovery']);

SELECT _qtss_register_key('onchain.discovery.ttl_hours', 'onchain','discovery','int',
    '48'::jsonb, NULL,
    'Hours since last_signal_at after which an onchain_discovery symbol is pruned.',
    'number', true, 'normal', ARRAY['onchain','discovery']);

SELECT _qtss_register_key('onchain.discovery.default_interval', 'onchain','discovery','string',
    '"1h"'::jsonb, NULL,
    'Bar interval to assign when inserting a discovered symbol into engine_symbols.',
    'text', true, 'normal', ARRAY['onchain','discovery']);

SELECT _qtss_register_key('onchain.pruner.enabled', 'onchain','discovery','bool',
    'true'::jsonb, NULL,
    'Enable the engine_symbols pruner loop. Only deletes source=onchain_discovery rows past TTL — never touches manual or top_volume.',
    'toggle', true, 'normal', ARRAY['onchain','discovery']);

SELECT _qtss_register_key('onchain.pruner.tick_interval_s', 'onchain','discovery','int',
    '3600'::jsonb, NULL,
    'How often the pruner runs (seconds).',
    'number', true, 'normal', ARRAY['onchain','discovery']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('onchain', 'discovery.enabled',          'false'::jsonb, 'Enable onchain discovery.'),
    ('onchain', 'discovery.tick_interval_s',  '900'::jsonb,   'Discovery loop tick (s).'),
    ('onchain', 'discovery.min_score',        '0.6'::jsonb,   'Discovery threshold.'),
    ('onchain', 'discovery.max_active',       '30'::jsonb,    'Discovery cap.'),
    ('onchain', 'discovery.ttl_hours',        '48'::jsonb,    'Discovery TTL (h).'),
    ('onchain', 'discovery.default_interval', '"1h"'::jsonb,  'Default bar interval for discovered symbols.'),
    ('onchain', 'pruner.enabled',             'true'::jsonb,  'Enable pruner.'),
    ('onchain', 'pruner.tick_interval_s',     '3600'::jsonb,  'Pruner tick (s).')
ON CONFLICT (module, config_key) DO NOTHING;
