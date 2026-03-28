-- Arka plan motorları: analiz edilecek semboller ve son snapshot (web GUI buradan okur).
-- Not: v14 `0014_acp_auto_scan_timeframe.sql` ile çakışmaması için 0015 numaralıdır.

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
    CONSTRAINT engine_symbols_unique_series UNIQUE (exchange, segment, symbol, interval)
);

CREATE INDEX IF NOT EXISTS idx_engine_symbols_enabled ON engine_symbols (enabled, sort_order, exchange, segment, symbol);

CREATE TABLE IF NOT EXISTS analysis_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols (id) ON DELETE CASCADE,
    engine_kind TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    last_bar_open_time TIMESTAMPTZ,
    bar_count INT,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT,
    CONSTRAINT analysis_snapshots_unique_target_kind UNIQUE (engine_symbol_id, engine_kind)
);

CREATE INDEX IF NOT EXISTS idx_analysis_snapshots_computed ON analysis_snapshots (computed_at DESC);
