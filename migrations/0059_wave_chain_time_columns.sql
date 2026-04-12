-- Elliott Deep: add time-range columns for cross-TF linking.
-- time_start/time_end = wall-clock times for bar_start/bar_end
-- so child waves on lower TFs can be matched by time range.

ALTER TABLE wave_chain ADD COLUMN IF NOT EXISTS time_start TIMESTAMPTZ;
ALTER TABLE wave_chain ADD COLUMN IF NOT EXISTS time_end   TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_wave_chain_time_range
    ON wave_chain (exchange, symbol, timeframe, degree, time_start, time_end)
    WHERE state != 'invalidated';

-- For ancestor chain queries (recursive CTE walking parent_id)
CREATE INDEX IF NOT EXISTS idx_wave_chain_detection
    ON wave_chain (detection_id) WHERE detection_id IS NOT NULL;
