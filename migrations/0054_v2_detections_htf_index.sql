-- Fix slow query: list_recent_for_symbol_htf uses (exchange, symbol, timeframe <>, state <> 'invalidated', confidence IS NOT NULL)
-- This partial index covers the common case efficiently.

CREATE INDEX IF NOT EXISTS qtss_v2_detections_htf_idx
    ON qtss_v2_detections (exchange, symbol, detected_at DESC)
    WHERE state <> 'invalidated' AND confidence IS NOT NULL;
