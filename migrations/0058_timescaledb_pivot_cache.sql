-- 0058: TimescaleDB hypertable for market_bars + pivot_cache table
--
-- TimescaleDB must already be installed (CREATE EXTENSION timescaledb).
-- This migration:
--   1. Converts market_bars to a hypertable (chunked by open_time)
--   2. Adds compression policy for old chunks
--   3. Creates pivot_cache for pre-computed pivot points

-- =========================================================================
-- 1. market_bars → hypertable
-- =========================================================================

-- TimescaleDB requires the time column in the PRIMARY KEY.
-- Current PK is (id), which is a UUID — not time-based.
-- We need to: drop PK, recreate with (id, open_time), then convert.

-- Drop FK constraints first (they reference this table's PK).
ALTER TABLE market_bars DROP CONSTRAINT IF EXISTS market_bars_bar_interval_id_fkey;
ALTER TABLE market_bars DROP CONSTRAINT IF EXISTS market_bars_instrument_id_fkey;

-- Drop old PK and recreate with open_time included.
ALTER TABLE market_bars DROP CONSTRAINT market_bars_pkey;
ALTER TABLE market_bars ADD PRIMARY KEY (id, open_time);

-- Convert to hypertable. chunk_time_interval = 1 month.
-- migrate_data => true moves existing rows into chunks.
SELECT create_hypertable(
    'market_bars',
    'open_time',
    chunk_time_interval => INTERVAL '1 month',
    migrate_data => true
);

-- Re-add FK constraints (SET NULL on delete, non-enforced in practice).
ALTER TABLE market_bars
    ADD CONSTRAINT market_bars_instrument_id_fkey
    FOREIGN KEY (instrument_id) REFERENCES instruments(id) ON DELETE SET NULL;
ALTER TABLE market_bars
    ADD CONSTRAINT market_bars_bar_interval_id_fkey
    FOREIGN KEY (bar_interval_id) REFERENCES bar_intervals(id) ON DELETE SET NULL;

-- =========================================================================
-- 2. Compression policy — compress chunks older than 6 months
-- =========================================================================

ALTER TABLE market_bars SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'exchange, segment, symbol, interval',
    timescaledb.compress_orderby = 'open_time DESC'
);

SELECT add_compression_policy('market_bars', INTERVAL '6 months');

-- =========================================================================
-- 3. pivot_cache — pre-computed pivots for historical data
-- =========================================================================

CREATE TABLE IF NOT EXISTS pivot_cache (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    exchange        TEXT NOT NULL,
    symbol          TEXT NOT NULL,
    timeframe       TEXT NOT NULL,
    level           TEXT NOT NULL,        -- 'L1', 'L2', 'L3', 'L4'
    bar_index       BIGINT NOT NULL,
    open_time       TIMESTAMPTZ NOT NULL,
    price           NUMERIC NOT NULL,
    kind            TEXT NOT NULL,        -- 'High' or 'Low'
    prominence      NUMERIC NOT NULL DEFAULT 1,
    volume_at_pivot NUMERIC NOT NULL DEFAULT 0,
    swing_type      TEXT,                 -- 'HH', 'HL', 'LH', 'LL' or NULL
    computed_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pivot_cache_unique UNIQUE (exchange, symbol, timeframe, level, bar_index)
);

CREATE INDEX idx_pivot_cache_series
    ON pivot_cache (exchange, symbol, timeframe, level, open_time DESC);

CREATE INDEX idx_pivot_cache_kind
    ON pivot_cache (exchange, symbol, timeframe, level, kind);

-- =========================================================================
-- 4. wave_chain — Elliott wave chain (placeholder for Elliott Deep phase)
-- =========================================================================

CREATE TABLE IF NOT EXISTS wave_chain (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    parent_id       UUID REFERENCES wave_chain(id) ON DELETE SET NULL,
    exchange        TEXT NOT NULL,
    symbol          TEXT NOT NULL,
    timeframe       TEXT NOT NULL,
    degree          TEXT NOT NULL,        -- 'primary', 'intermediate', 'minor', etc.
    kind            TEXT NOT NULL,        -- 'impulse', 'zigzag_abc', 'flat', 'triangle', etc.
    direction       TEXT NOT NULL,        -- 'bull' or 'bear'
    wave_number     TEXT,                 -- '1','2','3','4','5' or 'A','B','C' or 'W','X','Y'
    bar_start       BIGINT NOT NULL,
    bar_end         BIGINT NOT NULL,
    price_start     NUMERIC NOT NULL,
    price_end       NUMERIC NOT NULL,
    structural_score REAL NOT NULL DEFAULT 0,
    state           TEXT NOT NULL DEFAULT 'forming', -- 'forming', 'confirmed', 'invalidated'
    detection_id    UUID,                -- link to qtss_v2_detections
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_wave_chain_series
    ON wave_chain (exchange, symbol, timeframe, bar_start DESC);

CREATE INDEX idx_wave_chain_parent
    ON wave_chain (parent_id) WHERE parent_id IS NOT NULL;

COMMENT ON TABLE pivot_cache IS 'Pre-computed pivot points from historical bar data. Eliminates re-computation on every detection tick.';
COMMENT ON TABLE wave_chain IS 'Elliott wave chain — tracks parent-child degree relationships across timeframes. Populated by historical scan, extended by live detection.';
