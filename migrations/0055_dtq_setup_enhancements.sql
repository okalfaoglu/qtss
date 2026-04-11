-- Faz D/T/Q Setup Model enhancements.
--
-- 1. Add pnl_pct to setups for direct performance queries.
-- 2. Expand state CHECK to include closed_win / closed_loss / closed_manual.
-- 3. Add risk_mode column (tracks market regime at setup creation).
-- 4. Create Q-RADAR virtual portfolio table.

-- (a) pnl_pct + risk_mode columns
ALTER TABLE qtss_v2_setups
  ADD COLUMN IF NOT EXISTS pnl_pct REAL,
  ADD COLUMN IF NOT EXISTS risk_mode TEXT;

-- (b) Expand state CHECK constraint to granular close states.
-- Drop old constraint, add new one.
ALTER TABLE qtss_v2_setups DROP CONSTRAINT IF EXISTS qtss_v2_setups_state_check;
ALTER TABLE qtss_v2_setups
  ADD CONSTRAINT qtss_v2_setups_state_check
  CHECK (state IN ('flat','armed','active','closed','closed_win','closed_loss','closed_manual'));

-- (c) Q-RADAR virtual portfolio tracker.
CREATE TABLE IF NOT EXISTS q_radar_portfolio (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Snapshot of capital allocation
    total_capital REAL NOT NULL DEFAULT 1500000.0,
    allocated_capital REAL NOT NULL DEFAULT 0.0,
    available_capital REAL NOT NULL DEFAULT 1500000.0,
    -- Running P&L
    realized_pnl REAL NOT NULL DEFAULT 0.0,
    unrealized_pnl REAL NOT NULL DEFAULT 0.0,
    -- Counters
    open_positions INT NOT NULL DEFAULT 0,
    total_trades INT NOT NULL DEFAULT 0,
    win_trades INT NOT NULL DEFAULT 0,
    loss_trades INT NOT NULL DEFAULT 0
);

-- Single row portfolio (upsert pattern).
INSERT INTO q_radar_portfolio (id, total_capital, available_capital)
VALUES ('00000000-0000-0000-0000-000000000001', 1500000.0, 1500000.0)
ON CONFLICT (id) DO NOTHING;

-- (d) Q-RADAR position tracking per setup.
CREATE TABLE IF NOT EXISTS q_radar_positions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    setup_id UUID NOT NULL REFERENCES qtss_v2_setups(id) ON DELETE CASCADE,
    symbol TEXT NOT NULL,
    direction TEXT NOT NULL CHECK (direction IN ('long','short')),
    -- Capital allocated from the portfolio
    allocated_amount REAL NOT NULL,
    -- Lot / quantity management
    quantity REAL NOT NULL DEFAULT 0.0,
    avg_entry_price REAL NOT NULL DEFAULT 0.0,
    -- Partial sell / add-on tracking
    total_bought_qty REAL NOT NULL DEFAULT 0.0,
    total_sold_qty REAL NOT NULL DEFAULT 0.0,
    realized_pnl REAL NOT NULL DEFAULT 0.0,
    -- State
    state TEXT NOT NULL DEFAULT 'open' CHECK (state IN ('open','closed')),
    closed_at TIMESTAMPTZ,
    UNIQUE (setup_id)
);

-- (e) Q-RADAR position events (add-on buy, partial sell, close).
CREATE TABLE IF NOT EXISTS q_radar_position_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    position_id UUID NOT NULL REFERENCES q_radar_positions(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL CHECK (event_type IN ('open','add_on','partial_sell','close')),
    quantity REAL NOT NULL,
    price REAL NOT NULL,
    pnl REAL,
    notes TEXT,
    raw_meta JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_q_radar_positions_setup
  ON q_radar_positions (setup_id);
CREATE INDEX IF NOT EXISTS idx_q_radar_positions_open
  ON q_radar_positions (state) WHERE state = 'open';
CREATE INDEX IF NOT EXISTS idx_q_radar_position_events_pos
  ON q_radar_position_events (position_id, created_at DESC);
