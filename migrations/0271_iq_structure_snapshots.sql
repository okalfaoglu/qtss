-- 0271_iq_structure_snapshots.sql
-- Time-series snapshot of iq_structures state, one row per (symbol,
-- timeframe, bar_time). The live worker writes a row every time a
-- tracked structure advances; the backtest reads from this table
-- to reconstruct "what did the structural state look like at
-- bar_time T?" without depending on the point-in-time iq_structures
-- table (which only stores the CURRENT state).
--
-- This unblocks the backtest's `structural_completion` scorer for
-- historical windows. Without snapshots the scorer returned 0 for
-- every bar earlier than today (the tracker's last_advanced_at).
--
-- Schema mirrors the iq_structures fields the backtest scorer
-- reads:
--   - state (candidate / tracking / completed)
--   - current_wave (W1..W5 / A..C)
--   - raw_meta (projection branches, source pattern ids)
-- plus the bar_time pivot for time-range queries.

CREATE TABLE IF NOT EXISTS iq_structure_snapshots (
    exchange       TEXT NOT NULL,
    segment        TEXT NOT NULL,
    symbol         TEXT NOT NULL,
    timeframe      TEXT NOT NULL,
    slot           SMALLINT NOT NULL,
    bar_time       TIMESTAMPTZ NOT NULL,
    state          TEXT NOT NULL,
    current_wave   TEXT NOT NULL,
    direction      SMALLINT NOT NULL DEFAULT 0,
    raw_meta       JSONB NOT NULL DEFAULT '{}'::jsonb,
    captured_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (exchange, segment, symbol, timeframe, slot, bar_time)
);

-- Time-range scan index — backtest's structural_completion scorer
-- always queries by (sym, tf) + ORDER BY bar_time DESC LIMIT 1.
CREATE INDEX IF NOT EXISTS iq_structure_snapshots_time_idx
    ON iq_structure_snapshots (exchange, segment, symbol, timeframe, bar_time DESC);
