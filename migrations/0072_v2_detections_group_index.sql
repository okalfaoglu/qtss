-- 0072_v2_detections_group_index.sql
--
-- Hot-path index for the v2_detection_validator's historical outcome
-- aggregation:
--
--   SELECT family, subkind, timeframe,
--          COUNT(*) FILTER (WHERE confidence IS NOT NULL) AS validated,
--          COUNT(*) FILTER (WHERE state = 'invalidated')  AS invalidated
--     FROM qtss_v2_detections
--    GROUP BY family, subkind, timeframe;
--
-- With 10k+ detections this query hit the slow-statement log at
-- 2.2s on a seq-scan. A btree on the GROUP BY keys plus the two
-- filter columns lets Postgres do an index-only scan / merge group.
-- Including `confidence` and `state` as INCLUDE columns keeps the
-- FILTER aggregates off the heap.

CREATE INDEX IF NOT EXISTS idx_v2_detections_fst_outcome
    ON qtss_v2_detections (family, subkind, timeframe)
    INCLUDE (confidence, state);

COMMENT ON INDEX idx_v2_detections_fst_outcome IS
    'Covers the validator historical outcome GROUP BY (family, subkind, timeframe). INCLUDE columns allow index-only scans for the FILTER aggregates.';
