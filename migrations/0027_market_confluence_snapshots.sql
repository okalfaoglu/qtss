-- PLAN Phase B — derived confluence category scores (append-only history per engine target).
-- English JSON keys in scores_json: smart_money, cex_flow, dex_pressure, hyperliquid, funding_oi, liquidations, composite.

CREATE TABLE market_confluence_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    engine_symbol_id UUID NOT NULL REFERENCES engine_symbols (id) ON DELETE CASCADE,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    schema_version INT NOT NULL DEFAULT 1,
    regime TEXT,
    composite_score DOUBLE PRECISION NOT NULL,
    confidence_0_100 INT NOT NULL,
    scores_json JSONB NOT NULL,
    conflicts_json JSONB NOT NULL DEFAULT '[]'::jsonb
);

CREATE INDEX idx_market_confluence_snapshots_symbol_computed
    ON market_confluence_snapshots (engine_symbol_id, computed_at DESC);

COMMENT ON TABLE market_confluence_snapshots IS 'Append-only confluence score history (PLAN_CONFLUENCE_AND_MARKET_DATA Phase B).';
