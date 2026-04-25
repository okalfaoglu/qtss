-- Allocator v1.2.4 — confidence calibration: regular view → materialised.
--
-- 0250 introduced v_confidence_calibration as a plain VIEW. Every
-- allocator candidate hit it with a fresh aggregation over qtss_setups.
-- That was fine at low volume, but each closed-setup row is touched
-- by every candidate every tick — easy to amplify when the candidate
-- count or trade volume grows. Convert to a MATERIALIZED VIEW with a
-- background refresh; the read path (`SELECT ... FROM
-- v_confidence_calibration`) stays identical.
--
-- A unique index on `bucket` lets us use REFRESH MATERIALIZED VIEW
-- CONCURRENTLY so refresh does not block the allocator reads.

-- Drop the old object first (CREATE OR REPLACE doesn't switch a view
-- to a matview). The DO block handles both the original-view state
-- AND the already-promoted matview state (idempotent re-run).
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_matviews WHERE matviewname = 'v_confidence_calibration') THEN
        EXECUTE 'DROP MATERIALIZED VIEW v_confidence_calibration';
    ELSIF EXISTS (SELECT 1 FROM pg_views WHERE viewname = 'v_confidence_calibration') THEN
        EXECUTE 'DROP VIEW v_confidence_calibration';
    END IF;
END $$;

CREATE MATERIALIZED VIEW v_confidence_calibration AS
WITH closed AS (
    SELECT
        CASE
            WHEN ai_score IS NULL THEN NULL::text
            WHEN ai_score < 0.5 THEN '0.0-0.5'
            WHEN ai_score < 0.6 THEN '0.5-0.6'
            WHEN ai_score < 0.7 THEN '0.6-0.7'
            WHEN ai_score < 0.8 THEN '0.7-0.8'
            WHEN ai_score < 0.9 THEN '0.8-0.9'
            ELSE '0.9+'
        END AS bucket,
        CASE WHEN realized_pnl_pct > 0 THEN 1 ELSE 0 END AS win
    FROM qtss_setups
    WHERE closed_at IS NOT NULL
      AND realized_pnl_pct IS NOT NULL
      AND close_reason IN ('tp_final','sl_hit','trail_stop','invalidated')
)
SELECT
    bucket,
    COUNT(*)::bigint     AS total,
    SUM(win)::bigint     AS wins,
    CASE WHEN COUNT(*) = 0 THEN NULL
         ELSE SUM(win)::float8 / COUNT(*)::float8
    END AS winrate
FROM closed
WHERE bucket IS NOT NULL
GROUP BY bucket
ORDER BY bucket;

CREATE UNIQUE INDEX IF NOT EXISTS v_confidence_calibration_bucket_idx
    ON v_confidence_calibration (bucket);

COMMENT ON MATERIALIZED VIEW v_confidence_calibration IS
    'ChatGPT teardown #5: realized winrate per AI-score bucket. Materialised in v1.2.4; refreshed on a worker loop tick.';

-- Initial population so the allocator has data to read against
-- immediately after migration.
REFRESH MATERIALIZED VIEW v_confidence_calibration;

-- Refresh cadence — read by qtss-worker calibration_refresh_loop.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('calibration_refresh', 'enabled',
     '{"enabled": true}'::jsonb,
     'Master switch for the calibration matview refresh loop. When false the loop sleeps an hour and skips refreshing.'),
    ('calibration_refresh', 'tick_secs',
     '{"secs": 600}'::jsonb,
     'Refresh cadence in seconds. 600 = every 10 minutes — fast enough that a closed setup updates its bucket within the next allocator window, slow enough that the aggregation cost is negligible.')
ON CONFLICT (module, config_key) DO NOTHING;
