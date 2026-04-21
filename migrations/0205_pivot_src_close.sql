-- Faz 14.A17 — LuxAlgo 1-1 parity fix.
--
-- ZigZag pivot detection now uses CLOSE (matches Pine's default
-- `src = close` in `ta.highestbars(src, length)`) instead of high/low.
-- This yields a far sparser, more structural pivot set — the previous
-- high/low-triggered detection produced ~4× too many pivots vs.
-- LuxAlgo's visible output.
--
-- Pivot cache must be purged so the worker re-seeds with the new
-- close-based definition. All downstream pivot-derived detections
-- become invalid in the same way.

BEGIN;

DELETE FROM qtss_v2_detections
WHERE family IN ('elliott', 'harmonic', 'classical', 'pivot_reversal');

TRUNCATE pivot_cache;
TRUNCATE pivot_backfill_state;

COMMIT;
