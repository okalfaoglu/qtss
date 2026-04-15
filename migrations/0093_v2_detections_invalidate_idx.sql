-- 0093_v2_detections_invalidate_idx.sql
--
-- Slow-query fix: the orchestrator's "mark older same-pattern detections
-- as invalidated" UPDATE was tripping the 1s sqlx threshold:
--
--   UPDATE qtss_v2_detections
--      SET state = 'invalidated'
--    WHERE exchange = $1 AND symbol = $2 AND timeframe = $3
--      AND family = $4 AND subkind = $5
--      AND state IN ('forming','confirmed')
--      AND id <> $6;
--
-- Existing indexes (chart_idx, feed_idx, open_idx, htf_idx) don't cover
-- the full (exchange, symbol, timeframe, family, subkind) prefix, so the
-- planner falls back to a partial scan of open rows. Add a partial
-- composite that matches the WHERE exactly.

CREATE INDEX IF NOT EXISTS qtss_v2_detections_invalidate_idx
    ON qtss_v2_detections (exchange, symbol, timeframe, family, subkind)
    WHERE state IN ('forming','confirmed');
