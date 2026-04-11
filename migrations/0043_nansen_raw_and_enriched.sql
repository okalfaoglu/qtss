-- 0043: Raw Nansen flow data (AI-readable time series) + enriched signals.
--
-- nansen_raw_flows: every row from every Nansen API response is persisted
-- individually so AI/LLM can read historical on-chain flow patterns.
-- Rows are immutable (append-only); a retention job can prune old rows.
--
-- nansen_enriched_signals: derived signals computed by the enriched
-- analyzers (cross-chain flow, DEX volume spike, whale concentration).

CREATE TABLE IF NOT EXISTS nansen_raw_flows (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_type TEXT NOT NULL,             -- 'netflow', 'holdings', 'dex_trades', 'flow_intel'
    chain       TEXT,                      -- 'ethereum', 'solana', 'bnb', ...
    token_symbol TEXT,                     -- 'WBTC', 'WETH', 'SOL', ...
    token_address TEXT,                    -- '0x2260...' (nullable for native tokens)
    engine_symbol TEXT,                    -- mapped QTSS symbol: 'BTCUSDT' (nullable if unmapped)
    direction   TEXT,                      -- 'inflow', 'outflow', 'buy', 'sell', null
    value_usd   DOUBLE PRECISION,         -- net_flow / trade_value in USD
    balance_pct_change DOUBLE PRECISION,   -- 24h balance change % (holdings)
    raw_row     JSONB NOT NULL,            -- full original Nansen row
    snapshot_at TIMESTAMPTZ NOT NULL,      -- when Nansen produced this data
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_nrf_symbol_type ON nansen_raw_flows (engine_symbol, source_type, snapshot_at DESC);
CREATE INDEX idx_nrf_created ON nansen_raw_flows (created_at DESC);
CREATE INDEX idx_nrf_chain_token ON nansen_raw_flows (chain, token_symbol, snapshot_at DESC);

CREATE TABLE IF NOT EXISTS nansen_enriched_signals (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol         TEXT NOT NULL,
    signal_type    TEXT NOT NULL,           -- 'cross_chain_flow', 'dex_volume_spike', 'whale_concentration', 'smart_money_flow'
    score          DOUBLE PRECISION NOT NULL,
    direction      TEXT NOT NULL DEFAULT 'neutral',
    confidence     DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    chain_breakdown JSONB,                 -- per-chain scores { "ethereum": 0.4, "solana": -0.2 }
    details        JSONB,                  -- free-form metadata for AI/UI
    computed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_nes_symbol_type ON nansen_enriched_signals (symbol, signal_type, computed_at DESC);
