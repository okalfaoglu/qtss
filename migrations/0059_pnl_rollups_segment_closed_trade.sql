-- P&L rollup şeması: `qtss-storage::pnl` (segment filtre, rebuild INSERT, closed_trade_count).

ALTER TABLE pnl_rollups
    ADD COLUMN IF NOT EXISTS segment TEXT NOT NULL DEFAULT 'spot';

ALTER TABLE pnl_rollups
    ADD COLUMN IF NOT EXISTS closed_trade_count BIGINT NOT NULL DEFAULT 0;

-- Yeni PK segment içerir; NULL sembol tekilleştirmesi için boş metin.
UPDATE pnl_rollups SET symbol = '' WHERE symbol IS NULL;

ALTER TABLE pnl_rollups
    ALTER COLUMN symbol SET NOT NULL,
    ALTER COLUMN symbol SET DEFAULT '';

ALTER TABLE pnl_rollups DROP CONSTRAINT IF EXISTS pnl_rollups_pkey;

ALTER TABLE pnl_rollups ADD CONSTRAINT pnl_rollups_pkey PRIMARY KEY (
    org_id,
    exchange,
    segment,
    symbol,
    ledger,
    bucket,
    period_start
);
