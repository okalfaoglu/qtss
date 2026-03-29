-- Worker / analiz çekirdeği: `qtss-storage` + `qtss-worker` ile uyumlu tablolar.
-- Yeni kurulum: 0001–0012 sonrası. Mevcut DB: IF NOT EXISTS / ADD COLUMN IF NOT EXISTS.
-- Not: Eski ayrı `0013_bar_intervals.sql` ile çakışmayı önlemek için bar_intervals genişletmesi burada birleştirildi (tek SQLx sürümü 13).

ALTER TABLE markets DROP CONSTRAINT IF EXISTS markets_segment_check;

ALTER TABLE market_bars
    ADD COLUMN IF NOT EXISTS instrument_id UUID REFERENCES instruments (id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS bar_interval_id UUID;

CREATE TABLE IF NOT EXISTS bar_intervals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT NOT NULL,
    label TEXT,
    duration_seconds INTEGER,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT bar_intervals_code_key UNIQUE (code)
);

DO $$
BEGIN
    ALTER TABLE market_bars
        ADD CONSTRAINT market_bars_bar_interval_id_fkey FOREIGN KEY (bar_interval_id) REFERENCES bar_intervals (id) ON DELETE SET NULL;
EXCEPTION
    WHEN duplicate_object THEN NULL;
END
$$;

INSERT INTO bar_intervals (code, label, duration_seconds, sort_order, is_active, metadata)
VALUES
    ('1m', '1 minute', 60, 10, TRUE, '{}'),
    ('3m', '3 minutes', 180, 20, TRUE, '{}'),
    ('5m', '5 minutes', 300, 30, TRUE, '{}'),
    ('15m', '15 minutes', 900, 40, TRUE, '{}'),
    ('30m', '30 minutes', 1800, 50, TRUE, '{}'),
    ('1h', '1 hour', 3600, 60, TRUE, '{}'),
    ('2h', '2 hours', 7200, 70, TRUE, '{}'),
    ('4h', '4 hours', 14400, 80, TRUE, '{}'),
    ('1d', '1 day', 86400, 90, TRUE, '{}')
ON CONFLICT (code) DO NOTHING;

INSERT INTO bar_intervals (code, label, duration_seconds, sort_order, is_active, metadata) VALUES
    ('1s', '1 saniye', 1, 5, TRUE, '{}'),
    ('6h', '6 saat', 21600, 65, TRUE, '{}'),
    ('8h', '8 saat', 28800, 70, TRUE, '{}'),
    ('12h', '12 saat', 43200, 75, TRUE, '{}'),
    ('3d', '3 gün', 259200, 85, TRUE, '{}'),
    ('1w', '1 hafta', 604800, 90, TRUE, '{}'),
    ('1M', '1 ay', NULL, 100, TRUE, '{}')
ON CONFLICT (code) DO NOTHING;

CREATE INDEX IF NOT EXISTS idx_bar_intervals_active ON bar_intervals (is_active, sort_order);

COMMENT ON TABLE bar_intervals IS 'OHLC mum aralığı kataloğu; market_bars / engine_symbols FK ile tekrarlayan metin azaltılır.';

-- ACP tarama: Pine `ratioDiffEnabled` / `ratioDiff` — GUI varsayılanı ile hizalı (`analysis.rs` `default_acp_chart_patterns_json`).
UPDATE app_config
SET value = jsonb_set(
    value,
    '{scanning}',
    coalesce(value->'scanning', '{}'::jsonb)
      || '{"ratio_diff_enabled": false, "ratio_diff_max": 1.0}'::jsonb,
    true
  ),
  updated_at = now()
WHERE key = 'acp_chart_patterns'
  AND (value->'scanning'->'ratio_diff_enabled' IS NULL);

CREATE TABLE IF NOT EXISTS engine_symbols (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    exchange TEXT NOT NULL,
    segment TEXT NOT NULL,
    symbol TEXT NOT NULL,
    interval TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order INTEGER NOT NULL DEFAULT 0,
    label TEXT,
    signal_direction_mode TEXT NOT NULL DEFAULT 'auto_segment',
    exchange_id UUID REFERENCES exchanges (id) ON DELETE SET NULL,
    market_id UUID REFERENCES markets (id) ON DELETE SET NULL,
    instrument_id UUID REFERENCES instruments (id) ON DELETE SET NULL,
    bar_interval_id UUID REFERENCES bar_intervals (id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT engine_symbols_series_key UNIQUE (exchange, segment, symbol, interval)
);

ALTER TABLE engine_symbols
    ADD COLUMN IF NOT EXISTS exchange_id UUID REFERENCES exchanges (id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS market_id UUID REFERENCES markets (id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS instrument_id UUID REFERENCES instruments (id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS bar_interval_id UUID REFERENCES bar_intervals (id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS signal_direction_mode TEXT;

UPDATE engine_symbols
SET signal_direction_mode = 'auto_segment'
WHERE signal_direction_mode IS NULL;

ALTER TABLE engine_symbols
    ALTER COLUMN signal_direction_mode SET DEFAULT 'auto_segment';

ALTER TABLE engine_symbols
    ALTER COLUMN signal_direction_mode SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_engine_symbols_enabled ON engine_symbols (enabled)
WHERE
    enabled = TRUE;

CREATE TABLE IF NOT EXISTS analysis_snapshots (
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols (id) ON DELETE CASCADE,
    engine_kind TEXT NOT NULL,
    payload JSONB NOT NULL,
    last_bar_open_time TIMESTAMPTZ,
    bar_count INTEGER,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT,
    PRIMARY KEY (engine_symbol_id, engine_kind)
);

CREATE TABLE IF NOT EXISTS range_signal_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols (id) ON DELETE CASCADE,
    event_kind TEXT NOT NULL,
    bar_open_time TIMESTAMPTZ NOT NULL,
    reference_price DOUBLE PRECISION,
    source TEXT NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT range_signal_events_dedup UNIQUE (engine_symbol_id, event_kind, bar_open_time)
);

CREATE INDEX IF NOT EXISTS idx_range_signal_events_engine_time ON range_signal_events (engine_symbol_id, bar_open_time DESC);

CREATE TABLE IF NOT EXISTS data_snapshots (
    source_key TEXT PRIMARY KEY,
    request_json JSONB NOT NULL,
    response_json JSONB,
    meta_json JSONB,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT
);

CREATE TABLE IF NOT EXISTS nansen_snapshots (
    snapshot_kind TEXT PRIMARY KEY,
    request_json JSONB NOT NULL,
    response_json JSONB,
    meta_json JSONB,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    error TEXT
);

CREATE TABLE IF NOT EXISTS external_data_sources (
    key TEXT PRIMARY KEY,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    method TEXT NOT NULL,
    url TEXT NOT NULL,
    headers_json JSONB NOT NULL DEFAULT '{}',
    body_json JSONB,
    tick_secs INTEGER NOT NULL DEFAULT 30,
    description TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS onchain_signal_scores (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol TEXT NOT NULL,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    funding_score DOUBLE PRECISION,
    oi_score DOUBLE PRECISION,
    ls_ratio_score DOUBLE PRECISION,
    taker_vol_score DOUBLE PRECISION,
    exchange_netflow_score DOUBLE PRECISION,
    exchange_balance_score DOUBLE PRECISION,
    hl_bias_score DOUBLE PRECISION,
    hl_whale_score DOUBLE PRECISION,
    liquidation_score DOUBLE PRECISION,
    nansen_sm_score DOUBLE PRECISION,
    nansen_netflow_score DOUBLE PRECISION,
    nansen_perp_score DOUBLE PRECISION,
    nansen_buyer_quality_score DOUBLE PRECISION,
    tvl_trend_score DOUBLE PRECISION,
    aggregate_score DOUBLE PRECISION NOT NULL,
    confidence DOUBLE PRECISION NOT NULL,
    direction TEXT NOT NULL,
    market_regime TEXT,
    conflict_detected BOOLEAN NOT NULL DEFAULT FALSE,
    conflict_detail TEXT,
    snapshot_keys TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    meta_json JSONB
);

CREATE INDEX IF NOT EXISTS idx_onchain_signal_scores_symbol_time ON onchain_signal_scores (symbol, computed_at DESC);

CREATE TABLE IF NOT EXISTS market_confluence_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols (id) ON DELETE CASCADE,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    schema_version INTEGER NOT NULL,
    regime TEXT,
    composite_score DOUBLE PRECISION NOT NULL,
    confidence_0_100 INTEGER NOT NULL,
    scores_json JSONB NOT NULL,
    conflicts_json JSONB NOT NULL,
    confluence_payload_json JSONB
);

CREATE INDEX IF NOT EXISTS idx_market_confluence_engine_time ON market_confluence_snapshots (engine_symbol_id, computed_at DESC);

CREATE TABLE IF NOT EXISTS nansen_setup_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    request_json JSONB NOT NULL,
    source TEXT NOT NULL,
    candidate_count INTEGER NOT NULL,
    meta_json JSONB,
    error TEXT
);

CREATE INDEX IF NOT EXISTS idx_nansen_setup_runs_computed ON nansen_setup_runs (computed_at DESC);

CREATE TABLE IF NOT EXISTS nansen_setup_rows (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES nansen_setup_runs (id) ON DELETE CASCADE,
    rank INTEGER NOT NULL,
    chain TEXT NOT NULL,
    token_address TEXT NOT NULL,
    token_symbol TEXT NOT NULL,
    direction TEXT NOT NULL,
    score INTEGER NOT NULL,
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
    ohlc_enriched BOOLEAN NOT NULL DEFAULT FALSE,
    raw_metrics JSONB NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_nansen_setup_rows_run ON nansen_setup_rows (run_id, rank);

CREATE TABLE IF NOT EXISTS paper_balances (
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    quote_balance NUMERIC NOT NULL,
    base_positions JSONB NOT NULL DEFAULT '{}',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id)
);

CREATE TABLE IF NOT EXISTS paper_fills (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    exchange TEXT NOT NULL,
    segment TEXT NOT NULL,
    symbol TEXT NOT NULL,
    client_order_id UUID NOT NULL,
    side TEXT NOT NULL,
    quantity NUMERIC NOT NULL,
    avg_price NUMERIC NOT NULL,
    fee NUMERIC NOT NULL,
    quote_balance_after NUMERIC NOT NULL,
    base_positions_after JSONB NOT NULL,
    intent JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_paper_fills_user_created ON paper_fills (user_id, created_at DESC);
