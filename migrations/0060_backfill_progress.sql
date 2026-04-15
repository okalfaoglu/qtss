-- Tracks per-series backfill progress so the worker can:
--   1. Resume from where it left off after a crash/restart
--   2. Know when a series is fully backfilled (listing → now)
--   3. Verify completeness before running analysis

CREATE TABLE IF NOT EXISTS backfill_progress (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols(id) ON DELETE CASCADE,

    -- State machine: pending → backfilling → verifying → complete → live
    --   pending      = never started
    --   backfilling  = fetching historical data (may resume)
    --   verifying    = backfill done, running gap/count verification
    --   complete     = all historical bars present, verified
    --   live         = complete + real-time updates active
    state           TEXT NOT NULL DEFAULT 'pending',

    -- Resume cursor: the oldest open_time we've fetched so far.
    -- Backfill resumes from here backwards.
    oldest_fetched  TIMESTAMPTZ,

    -- The newest bar we have (usually ~ now).
    newest_fetched  TIMESTAMPTZ,

    -- Bar counts for completeness check
    bar_count       BIGINT NOT NULL DEFAULT 0,
    expected_bars   BIGINT,              -- computed from interval & time span

    -- Gap tracking
    gap_count       INT NOT NULL DEFAULT 0,
    max_gap_seconds INT,

    -- Audit
    backfill_started_at  TIMESTAMPTZ,
    backfill_finished_at TIMESTAMPTZ,
    verified_at          TIMESTAMPTZ,
    last_error           TEXT,
    pages_fetched        INT NOT NULL DEFAULT 0,
    bars_upserted        BIGINT NOT NULL DEFAULT 0,

    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT backfill_progress_unique UNIQUE (engine_symbol_id)
);

CREATE INDEX IF NOT EXISTS idx_backfill_progress_state ON backfill_progress(state);

-- Seed rows for all existing engine_symbols
INSERT INTO backfill_progress (engine_symbol_id, state)
SELECT id, 'pending'
FROM engine_symbols
WHERE id NOT IN (SELECT engine_symbol_id FROM backfill_progress)
ON CONFLICT DO NOTHING;

-- Also create the ingestion_state table if missing (used by health metrics)
CREATE TABLE IF NOT EXISTS engine_symbol_ingestion_state (
    engine_symbol_id UUID PRIMARY KEY REFERENCES engine_symbols(id) ON DELETE CASCADE,
    bar_row_count    INT,
    min_open_time    TIMESTAMPTZ,
    max_open_time    TIMESTAMPTZ,
    gap_count        INT DEFAULT 0,
    max_gap_seconds  INT,
    last_backfill_at TIMESTAMPTZ,
    last_health_check_at TIMESTAMPTZ,
    last_error       TEXT,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
