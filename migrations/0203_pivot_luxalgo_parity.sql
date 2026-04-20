-- 0203_pivot_luxalgo_parity.sql
--
-- Faz 14.A12 — LuxAlgo-ZigZag parity pivot re-seed.
--
-- The previous ATR multipliers [L0: 2, L1: 3, L2: 5, L3: 8] put L0
-- above typical wave-2 / wave-4 retrace ATR ranges (1-1.5× ATR), so
-- the Elliott detector was structurally starved: pivots that exist on
-- the chart never entered the tree and full 6-pivot impulses could not
-- assemble. New defaults ship in `PivotConfig::defaults()` and are:
--
--     atr_mult      = [1.0, 2.0, 3.5, 6.0]
--     min_hold_bars = [1,   2,   3,   5  ]
--
-- The worker reads these from code (no config_value rows exist for
-- these keys in this environment), so the code-level change is the
-- authoritative switch. What this migration does is invalidate the
-- downstream caches so the backfill loop recomputes with the new
-- thresholds from scratch — otherwise the worker would keep mixing
-- old-threshold pivots with new detections.
--
-- Live detections from families that don't consume the pivot tree
-- (wyckoff / range / tbm) are left intact.

BEGIN;

-- Drop every detection whose anchors were pinned to stale pivots.
-- The orchestrator will re-emit on the next pass.
DELETE FROM qtss_v2_detections
 WHERE family IN ('elliott', 'harmonic', 'classical', 'pivot_reversal');

-- Clear the pivot cache so `pivot_historical_backfill` re-scans bars
-- from the start with the new (smaller) ATR multipliers.
TRUNCATE pivot_cache;
-- Reset the backfill watermark so the historical scan starts from bar 0
-- again. Without this the loop would fast-forward past the blank cache.
TRUNCATE pivot_backfill_state;

COMMIT;
