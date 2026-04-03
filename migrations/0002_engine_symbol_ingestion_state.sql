-- Worker REST backfill health (`qtss-storage` ingestion_state, `GET …/analysis/engine/ingestion-state`).
-- Was referenced in Rust before any migration created the table (500: relation does not exist).

CREATE TABLE IF NOT EXISTS engine_symbol_ingestion_state (
    engine_symbol_id UUID NOT NULL PRIMARY KEY REFERENCES engine_symbols (id) ON DELETE CASCADE,
    bar_row_count INTEGER NOT NULL DEFAULT 0,
    min_open_time TIMESTAMPTZ,
    max_open_time TIMESTAMPTZ,
    gap_count INTEGER NOT NULL DEFAULT 0,
    max_gap_seconds INTEGER,
    last_backfill_at TIMESTAMPTZ,
    last_health_check_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
