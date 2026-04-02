-- Setup taraması: `setup_scan_engine` → `insert_nansen_setup_run` / `insert_nansen_setup_row`.
-- Idempotent: `0013_worker_analytics_schema.sql` may already create these tables.

CREATE TABLE IF NOT EXISTS nansen_setup_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    request_json JSONB NOT NULL,
    source TEXT NOT NULL,
    candidate_count INT NOT NULL,
    meta_json JSONB,
    error TEXT
);

CREATE INDEX IF NOT EXISTS idx_nansen_setup_runs_computed ON nansen_setup_runs (computed_at DESC);

CREATE TABLE IF NOT EXISTS nansen_setup_rows (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES nansen_setup_runs (id) ON DELETE CASCADE,
    rank INT NOT NULL,
    chain TEXT NOT NULL,
    token_address TEXT NOT NULL,
    token_symbol TEXT NOT NULL,
    direction TEXT NOT NULL,
    score INT NOT NULL,
    probability DOUBLE PRECISION NOT NULL,
    setup TEXT NOT NULL,
    key_signals JSONB NOT NULL,
    entry DOUBLE PRECISION NOT NULL,
    stop_loss DOUBLE PRECISION NOT NULL,
    tp1 DOUBLE PRECISION NOT NULL,
    tp2 DOUBLE PRECISION NOT NULL,
    tp3 DOUBLE PRECISION NOT NULL,
    rr DOUBLE PRECISION NOT NULL,
    pct_to_tp2 DOUBLE PRECISION NOT NULL,
    ohlc_enriched BOOLEAN NOT NULL DEFAULT false,
    raw_metrics JSONB NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_nansen_setup_rows_run ON nansen_setup_rows (run_id, rank);
