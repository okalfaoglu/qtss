-- Re-key `detections` idempotency on (start_time, end_time) instead of
-- (start_bar, end_bar). bar_index is unstable across writer ticks: the
-- worker's 2000-bar window shifts every 60s, so the same physical
-- pattern gets a new bar_index each tick → upsert creates duplicates.
-- start_time / end_time are the pattern's real-world anchors and don't
-- drift, so keying on them keeps detections idempotent across runs.

-- Truncate accumulated duplicates first — the writer repopulates the
-- table in one tick (worst case: 60s of empty table).
TRUNCATE TABLE detections;

-- Swap the unique constraint.
ALTER TABLE detections DROP CONSTRAINT IF EXISTS detections_unique_span;
ALTER TABLE detections ADD CONSTRAINT detections_unique_span UNIQUE
    (exchange, segment, symbol, timeframe, slot,
     pattern_family, subkind, start_time, end_time, mode);
