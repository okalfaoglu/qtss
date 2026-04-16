-- 0118_faz9_training_set_view.sql
--
-- Faz 9.2.1 — Training set surface.
--
-- Joins (setup × outcome × feature snapshots) into a single readable view
-- so the Python trainer (Faz 9.3) issues one query and pandas/polars
-- does the rest. `features_by_source` is a JSONB map keyed by
-- ConfluenceSource slug (wyckoff / derivatives / ...).
--
-- Features are written at detection time (setup_id NULL initially), so the
-- canonical join path is `qtss_features_snapshot.detection_id = qtss_v2_setups.detection_id`.

CREATE OR REPLACE VIEW v_qtss_training_set AS
SELECT
    s.id                     AS setup_id,
    s.detection_id           AS detection_id,
    s.venue_class,
    s.exchange,
    s.symbol,
    s.timeframe,
    s.profile,
    s.direction,
    s.state,
    s.created_at,
    s.closed_at,
    s.confluence_id,
    s.risk_mode,
    s.mode,
    o.label,
    o.close_reason,
    o.close_reason_category  AS category,
    o.realized_rr,
    o.pnl_pct                AS outcome_pnl_pct,
    o.max_favorable_r,
    o.max_adverse_r,
    o.time_to_outcome_bars,
    o.bars_to_first_tp       AS outcome_bars_to_first_tp,
    (
        SELECT jsonb_object_agg(fs.source, fs.features_json)
        FROM qtss_features_snapshot fs
        WHERE fs.detection_id = s.detection_id
    ) AS features_by_source,
    (
        SELECT COALESCE(array_agg(DISTINCT fs.source ORDER BY fs.source), ARRAY[]::text[])
        FROM qtss_features_snapshot fs
        WHERE fs.detection_id = s.detection_id
    ) AS feature_sources
FROM qtss_v2_setups s
LEFT JOIN qtss_setup_outcomes o ON o.setup_id = s.id;

COMMENT ON VIEW v_qtss_training_set IS
  'Faz 9.2.1 — single-row-per-setup training surface; features_by_source is a JSONB map keyed by ConfluenceSource slug.';

-- Convenience: closed-only slice the LightGBM trainer actually consumes.
CREATE OR REPLACE VIEW v_qtss_training_set_closed AS
SELECT * FROM v_qtss_training_set
WHERE closed_at IS NOT NULL
  AND label IS NOT NULL;

COMMENT ON VIEW v_qtss_training_set_closed IS
  'Faz 9.2.1 — training set filtered to setups that have a labeled outcome (the Faz 9.3 trainer input).';
