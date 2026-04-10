-- 0039 — account_drawdown_snapshots: periodic drawdown state persistence.
-- The worker snapshots peak/current equity at each risk tick.
-- Survives restarts; feeds drawdown history chart + bootstrap.

CREATE TABLE IF NOT EXISTS account_drawdown_snapshots (
    id              BIGSERIAL PRIMARY KEY,
    user_id         UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    exchange        TEXT NOT NULL DEFAULT 'binance',
    peak_equity     NUMERIC(24,8) NOT NULL,
    current_equity  NUMERIC(24,8) NOT NULL,
    drawdown_pct    NUMERIC(8,5) NOT NULL,
    snapped_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_drawdown_user_time
    ON account_drawdown_snapshots (user_id, exchange, snapped_at DESC);

-- Thin out old snapshots: keep only 1 per hour after 7 days, 1 per day after 30 days.
-- Handled by a scheduled cleanup job, not a trigger (avoid locking on insert path).
