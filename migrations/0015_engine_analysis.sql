-- Arka plan motor hedefleri + analiz snapshot’ları (`qtss-worker` engine_analysis, confluence).
-- `0013_worker_analytics_schema.sql` zaten `engine_symbols` / `analysis_snapshots` oluşturabildiği için IF NOT EXISTS.

CREATE TABLE IF NOT EXISTS engine_symbols (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    exchange TEXT NOT NULL DEFAULT 'binance',
    segment TEXT NOT NULL DEFAULT 'spot',
    symbol TEXT NOT NULL,
    interval TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    sort_order INT NOT NULL DEFAULT 0,
    label TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (exchange, segment, symbol, interval)
);

CREATE INDEX IF NOT EXISTS idx_engine_symbols_symbol ON engine_symbols (symbol);
CREATE INDEX IF NOT EXISTS idx_engine_symbols_enabled ON engine_symbols (enabled) WHERE enabled = true;

CREATE TABLE IF NOT EXISTS analysis_snapshots (
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols (id) ON DELETE CASCADE,
    engine_kind TEXT NOT NULL,
    payload JSONB NOT NULL,
    last_bar_open_time TIMESTAMPTZ,
    bar_count INT,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT,
    PRIMARY KEY (engine_symbol_id, engine_kind)
);

CREATE INDEX IF NOT EXISTS idx_analysis_snapshots_kind ON analysis_snapshots (engine_kind);
