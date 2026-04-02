-- Per engine_symbols target: worker-maintained market_bars coverage, gaps, backfill timestamps.
CREATE TABLE IF NOT EXISTS engine_symbol_ingestion_state (
    engine_symbol_id UUID PRIMARY KEY REFERENCES engine_symbols (id) ON DELETE CASCADE,
    bar_row_count INTEGER NOT NULL DEFAULT 0,
    min_open_time TIMESTAMPTZ,
    max_open_time TIMESTAMPTZ,
    gap_count INTEGER NOT NULL DEFAULT 0,
    max_gap_seconds INTEGER,
    last_backfill_at TIMESTAMPTZ,
    last_health_check_at TIMESTAMPTZ,
    last_error TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_engine_symbol_ingestion_gap
    ON engine_symbol_ingestion_state (gap_count)
    WHERE gap_count > 0;

COMMENT ON TABLE engine_symbol_ingestion_state IS
    'Worker: market_bars row count, min/max open_time, recent gap estimate, last REST backfill and health check per engine_symbols row.';
COMMENT ON TABLE engine_symbols IS
    'Watched symbols with processing potential (spot or futures). Worker keeps market_bars history (REST) and live closed klines (WS) aligned per row for Binance.';
