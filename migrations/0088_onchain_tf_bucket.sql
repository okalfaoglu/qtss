-- 0088_onchain_tf_bucket.sql
--
-- Faz 7.7 / P29b — per-TF onchain aggregate split.
--
-- Problem: `qtss_v2_onchain_metrics` stored one blended row per symbol,
-- mixing fast (1h derivatives) and slow (24h stablecoin / nansen-delta)
-- readings. A 15m TBM setup therefore inherited yesterday's slow bias,
-- freezing the aggregate score and direction.
--
-- Fix: each pass now writes two rows per symbol:
--   * tf_bucket='ltf' — only readings with cadence_s <= 3600
--   * tf_bucket='htf' — every reading (backwards-compatible blend)
-- The TBM bridge picks the bucket that matches the detector's analysis
-- timeframe (see P29c).

ALTER TABLE qtss_v2_onchain_metrics
    ADD COLUMN IF NOT EXISTS tf_bucket TEXT NOT NULL DEFAULT 'htf';

-- Bucket guard: only the two buckets the worker emits + legacy 'all'
-- rows written before this migration.
ALTER TABLE qtss_v2_onchain_metrics
    DROP CONSTRAINT IF EXISTS qtss_v2_onchain_metrics_tf_bucket_chk;
ALTER TABLE qtss_v2_onchain_metrics
    ADD CONSTRAINT qtss_v2_onchain_metrics_tf_bucket_chk
        CHECK (tf_bucket IN ('ltf','htf','all'));

-- Retire the old single-dimension index in favour of a bucket-aware one
-- so `fetch_latest_for_tbm_bucket` stays O(log n) per (symbol, bucket).
DROP INDEX IF EXISTS qtss_v2_onchain_metrics_symbol_time_idx;
CREATE INDEX IF NOT EXISTS qtss_v2_onchain_metrics_symbol_bucket_time_idx
    ON qtss_v2_onchain_metrics (symbol, tf_bucket, computed_at DESC);

-- ---------------------------------------------------------------------
-- system_config seed: the ltf/htf cadence cutoff. Operators can tune
-- this without a redeploy (CLAUDE.md #2) when fetcher cadences change.
-- ---------------------------------------------------------------------
SELECT _qtss_register_key('onchain.aggregator.ltf_cadence_s', 'onchain','aggregator','int',
    '3600'::jsonb, 'seconds',
    'Maximum reading cadence (seconds) that contributes to the ltf onchain aggregate. Slower readings only feed htf.',
    'number', true, 'normal', ARRAY['onchain','aggregator']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('onchain', 'aggregator.ltf_cadence_s', '3600'::jsonb,
     'Max reading cadence (s) that contributes to the ltf onchain aggregate.')
ON CONFLICT (module, config_key) DO NOTHING;
