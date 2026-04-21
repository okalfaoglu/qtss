-- Faz 14.A18 — LuxAlgo `ta.pivothigh/pivotlow` 1-1 parity.
--
-- ZigZag detection rewritten to match LuxAlgo Elliott Waves Pine source
-- exactly: two-sided strict pivot (left=length, right=1) on the
-- high/low series (not close, not one-sided). The previous two attempts
-- (one-sided highestbars; close-triggered) were both wrong — Pine uses
-- `ta.pivothigh(high, length, 1)`. Purge and reseed.

BEGIN;

DELETE FROM qtss_v2_detections
WHERE family IN ('elliott', 'harmonic', 'classical', 'pivot_reversal');

TRUNCATE pivot_cache;
TRUNCATE pivot_backfill_state;

COMMIT;
