-- Prevent duplicate open setups for the same (exchange, symbol, timeframe, profile, direction).
-- This closes a TOCTOU race condition in try_arm_new_setup where two concurrent
-- ticks could both pass the application-level duplicate check and insert two setups.

CREATE UNIQUE INDEX IF NOT EXISTS uq_open_setup_key
    ON qtss_v2_setups (exchange, symbol, timeframe, profile, direction)
    WHERE state IN ('armed', 'open');
