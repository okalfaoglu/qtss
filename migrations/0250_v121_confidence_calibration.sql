-- Allocator v1.2.1 — confidence calibration (ChatGPT teardown #5).
--
-- "confidence ≥ 0.65" is meaningless without knowing what that score
-- translates to as a REAL win rate. Raw confidence can be arbitrarily
-- scaled by the scorer; the calibration view computes the realized
-- win rate per confidence bucket so the allocator can gate on a true
-- probability estimate instead of a raw score.
--
-- View refreshed per query (not materialised) since the setup
-- population is small enough that a full scan is cheap. Can be
-- converted to a materialised view + scheduled REFRESH later if
-- the trade volume grows.

CREATE OR REPLACE VIEW v_confidence_calibration AS
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

COMMENT ON VIEW v_confidence_calibration IS
    'ChatGPT teardown #5: realized winrate per AI-score bucket. Allocator uses this to convert raw confidence into a true probability estimate before gating.';

-- Config seed — the minimum CALIBRATED winrate a candidate must meet
-- to pass the calibration gate. Separate from the raw-confidence gate
-- so operators can run both checks side-by-side during rollout.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('allocator_v2', 'calibration.enabled',
     '{"enabled": true}'::jsonb,
     'Master on/off for the calibration gate. When true, allocator maps raw confidence to the realized winrate of its bucket and rejects when below calibration.min_winrate.'),
    ('allocator_v2', 'calibration.min_winrate',
     '{"value": 0.45}'::jsonb,
     'Minimum bucket winrate required to pass the calibration gate. 0.45 leaves headroom for RR ≥ 2.0 setups to stay net-positive.'),
    ('allocator_v2', 'calibration.min_sample',
     '{"value": 20}'::jsonb,
     'Sample-size floor for a bucket to be "trusted" — below this, the gate is skipped and the raw confidence gate stands alone (cold-start protection).')
ON CONFLICT (module, config_key) DO NOTHING;
