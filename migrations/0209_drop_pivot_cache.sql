-- 0209 — Retire pivot_cache and pivot_backfill_state.
--
-- `pivots` (migration 0205) is the canonical pivot store. Every reader has
-- been migrated (pivot_reversal orchestrator, v2_pivots API, classical /
-- elliott / harmonic / wyckoff / pivot_reversal backtest sweeps, reset
-- binary). The `pivot_historical_backfill_loop` has been removed from the
-- worker — `pivot_writer_loop` writes directly to `pivots` from live
-- zigzag confirmations, so the separate backfill cursor is no longer
-- needed.
--
-- `prominence` was added to `pivots` in 0208 and is populated by
-- `pivot_writer_loop`, closing the last feature gap that kept pivot_cache
-- around during the transition (CLAUDE.md #5 — single source of truth).

DROP TABLE IF EXISTS pivot_cache CASCADE;
DROP TABLE IF EXISTS pivot_backfill_state CASCADE;
