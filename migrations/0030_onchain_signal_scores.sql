-- SPEC_ONCHAIN_SIGNALS §3.2 — önceki yanlış sürüm: 0014_onchain_signal_scores.sql (0014_acp_auto_scan_timeframe ile çakışıyordu).

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
    tvl_trend_score DOUBLE PRECISION,
    aggregate_score DOUBLE PRECISION NOT NULL,
    confidence DOUBLE PRECISION NOT NULL DEFAULT 0.5,
    direction TEXT NOT NULL CHECK (direction IN
        ('strong_buy','buy','neutral','sell','strong_sell')),
    market_regime TEXT,
    conflict_detected BOOLEAN NOT NULL DEFAULT false,
    conflict_detail TEXT,
    snapshot_keys TEXT[] NOT NULL DEFAULT '{}',
    meta_json JSONB
);

CREATE INDEX IF NOT EXISTS idx_ocs_symbol_time ON onchain_signal_scores (symbol, computed_at DESC);

COMMENT ON TABLE onchain_signal_scores IS 'SPEC_ONCHAIN_SIGNALS — qtss-worker onchain_signal_scorer';
