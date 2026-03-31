-- Add `segment` to pnl_rollups to avoid spot/futures mixing.

ALTER TABLE pnl_rollups
ADD COLUMN segment TEXT;

-- Backfill legacy rows (best effort). Live rebuild will overwrite live ledger anyway.
UPDATE pnl_rollups
SET segment = COALESCE(NULLIF(segment, ''), 'spot')
WHERE segment IS NULL OR segment = '';

ALTER TABLE pnl_rollups
ALTER COLUMN segment SET NOT NULL;

-- Primary key must include segment.
ALTER TABLE pnl_rollups
DROP CONSTRAINT IF EXISTS pnl_rollups_pkey;

ALTER TABLE pnl_rollups
ADD PRIMARY KEY (org_id, exchange, segment, symbol, ledger, bucket, period_start);

DROP INDEX IF EXISTS idx_pnl_ledger_bucket;
CREATE INDEX idx_pnl_ledger_bucket ON pnl_rollups (ledger, bucket, period_start DESC);
CREATE INDEX idx_pnl_instrument ON pnl_rollups (exchange, segment, symbol);

