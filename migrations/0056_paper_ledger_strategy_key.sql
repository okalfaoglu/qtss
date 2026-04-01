-- Paper ledger: per-strategy rows for parallel dry runners (`PaperRecordingDryGateway`).
-- Baseline tables: `0017_paper_ledger.sql` (user_id PK, fills without strategy_key).
-- Idempotent: safe if re-run or if composite PK already exists.

ALTER TABLE paper_balances
    ADD COLUMN IF NOT EXISTS strategy_key TEXT NOT NULL DEFAULT 'default';

DO $$
BEGIN
    ALTER TABLE paper_balances DROP CONSTRAINT paper_balances_pkey;
EXCEPTION
    WHEN undefined_object THEN NULL;
END $$;

DO $$
BEGIN
    ALTER TABLE paper_balances ADD PRIMARY KEY (user_id, strategy_key);
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

CREATE INDEX IF NOT EXISTS idx_paper_balances_org ON paper_balances (org_id);

ALTER TABLE paper_fills
    ADD COLUMN IF NOT EXISTS strategy_key TEXT NOT NULL DEFAULT 'default';

CREATE INDEX IF NOT EXISTS idx_paper_fills_strategy ON paper_fills (user_id, strategy_key, created_at DESC);
