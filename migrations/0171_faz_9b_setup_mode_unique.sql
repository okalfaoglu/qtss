-- 0171_faz_9b_setup_mode_unique.sql
--
-- Faz 9B — backfill mode propagation fix.
--
-- Problem: qtss_setups.uq_open_setup_key uniqued on (exchange, symbol,
-- timeframe, profile) without mode. A live setup in 'active' state would
-- block the backfill orchestrator from creating a concurrent 'backtest'
-- setup for the same key — historical_progressive_scan would lose every
-- backtest replay row and the training set would never grow.
--
-- Fix: widen the partial unique index to include mode so live + backtest
-- (+ dry) can coexist per (exchange, symbol, timeframe, profile).
--
-- Idempotent.

DROP INDEX IF EXISTS uq_open_setup_key;

CREATE UNIQUE INDEX IF NOT EXISTS uq_open_setup_key
  ON qtss_setups (exchange, symbol, timeframe, profile, mode)
  WHERE state IN ('armed', 'active');
