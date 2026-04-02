-- Dry / paper ledger (`PaperLedgerRepository`).
-- Idempotent: `0013_worker_analytics_schema.sql` may already create these tables.

CREATE TABLE IF NOT EXISTS paper_balances (
    user_id UUID NOT NULL PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    org_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    quote_balance NUMERIC NOT NULL,
    base_positions JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
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
CREATE INDEX IF NOT EXISTS idx_paper_fills_created ON paper_fills (created_at);
