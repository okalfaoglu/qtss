-- 0107_detection_outcome_stats_mv.sql
--
-- Perf fix — `historical_outcome_counts()` was scanning all 11M rows of
-- `qtss_v2_detections` on every validator tick (≈1.2 s). The result
-- (≈391 group rows) is a cheap proxy for hit-rate and does not need to
-- be realtime. Cache it in a MATERIALIZED VIEW refreshed out-of-band.

CREATE MATERIALIZED VIEW IF NOT EXISTS qtss_v2_detection_outcome_stats AS
    SELECT family,
           subkind,
           timeframe,
           COUNT(*) FILTER (WHERE confidence IS NOT NULL) AS validated_count,
           COUNT(*) FILTER (WHERE state = 'invalidated')  AS invalidated_count
      FROM qtss_v2_detections
     GROUP BY family, subkind, timeframe
    WITH DATA;

-- CONCURRENTLY refreshes need a unique index.
CREATE UNIQUE INDEX IF NOT EXISTS qtss_v2_detection_outcome_stats_pk
    ON qtss_v2_detection_outcome_stats (family, subkind, timeframe);

COMMENT ON MATERIALIZED VIEW qtss_v2_detection_outcome_stats IS
    'Cheap hit-rate proxy for qtss_v2_detections. Refreshed by the worker every N minutes via REFRESH MATERIALIZED VIEW CONCURRENTLY.';
