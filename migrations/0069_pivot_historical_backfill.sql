-- 0069_pivot_historical_backfill.sql — Faz 10 / P2a.
--
-- Shared pivot historical backfill.
--
-- The live v2_detection_orchestrator rebuilds PivotEngine every tick
-- from the last `history_bars` (default 500) bars — which means:
--   1) pivots older than that window never get cached; and
--   2) `bar_index` written to pivot_cache is relative to the rolling
--      window, not the full series, so the same pivot gets duplicated
--      under different bar_index values as the window slides.
--
-- Consequences:
--   * Wyckoff / Elliott / Classical detectors never see phases that
--     completed before the window — faz-A events in history are
--     permanently invisible.
--   * Backtest mode has no reliable historical pivot stream to replay.
--
-- The pivot_historical_backfill worker solves both by:
--   1. Iterating every bar from the very first stored candle, ASC.
--   2. Feeding a single fresh PivotEngine so bar_index is GLOBAL
--      (= 0 at the first bar of the symbol × timeframe series).
--   3. Wiping prior cache rows for the series (they were inconsistent)
--      and re-writing a clean, complete L0..L3 pivot set.
--   4. Recording completion in `pivot_backfill_state` so subsequent
--      ticks only re-run when new bars accumulate past the cursor.
--
-- All detectors read from `pivot_cache` → a correct backfill here fixes
-- Wyckoff phase detection (Faz 10 #1), Elliott historical context, and
-- enables deterministic backtests.

-- =========================================================================
-- 1. Per-series backfill state
-- =========================================================================

CREATE TABLE IF NOT EXISTS pivot_backfill_state (
    exchange        TEXT        NOT NULL,
    segment         TEXT        NOT NULL,
    symbol          TEXT        NOT NULL,
    timeframe       TEXT        NOT NULL,
    last_open_time  TIMESTAMPTZ NOT NULL,
    bars_processed  BIGINT      NOT NULL,
    pivots_written  BIGINT      NOT NULL,
    completed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (exchange, segment, symbol, timeframe)
);

COMMENT ON TABLE pivot_backfill_state IS
    'Cursor per (exchange, segment, symbol, timeframe) for the pivot historical backfill worker. last_open_time = newest bar already folded into pivot_cache with global bar_index semantics.';

-- =========================================================================
-- 2. Worker config keys
-- =========================================================================

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('worker', 'pivot_backfill_enabled', '{"enabled":true}',
   'Enable the pivot historical backfill worker. Backfills L0..L3 pivots from the first stored bar so detectors and backtests see full history.'),
  ('worker', 'pivot_backfill_tick_secs', '3600',
   'Poll interval for the pivot historical backfill worker. Hourly is enough — the work is idempotent and only runs when new bars accumulate past the per-series cursor.'),
  ('detector', 'pivot_backfill.chunk_bars', '5000',
   'Chunk size when paging through market_bars during backfill. Larger = fewer round-trips, more memory per symbol.'),
  ('detector', 'pivot_backfill.min_bars', '60',
   'Skip series with fewer than N bars — not enough to produce meaningful pivots.')
ON CONFLICT (module, config_key) DO NOTHING;

-- NOTE on semantics: ATR + zigzag legs are stateful, so a correct replay
-- must start from bar 0. When new bars accumulate past the cursor we
-- do a FULL rebuild (delete prior cache rows for the series, re-feed
-- every bar). Cost is acceptable — backfill ticks hourly and only when
-- the cursor is stale.
