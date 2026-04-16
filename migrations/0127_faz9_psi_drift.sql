-- 0127_faz9_psi_drift.sql
--
-- Faz 9.4.2 — PSI / Feature Drift Monitor.
--
-- Stores per-feature Population Stability Index snapshots so the
-- shadow dashboard can surface distribution shift between training
-- and live inference features.

CREATE TABLE IF NOT EXISTS qtss_ml_drift_snapshots (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    model_version   TEXT        NOT NULL,
    feature_name    TEXT        NOT NULL,
    psi             REAL        NOT NULL,
    bucket_detail   JSONB,
    computed_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_drift_snap_model_ts
    ON qtss_ml_drift_snapshots (model_version, computed_at DESC);

CREATE INDEX IF NOT EXISTS idx_drift_snap_feature_ts
    ON qtss_ml_drift_snapshots (feature_name, computed_at DESC);

-- Config keys for PSI thresholds and compute interval.

SELECT _qtss_register_key(
    'ai.drift.psi_warning_threshold',
    'ai',
    'ai',
    'float',
    '0.1'::jsonb,
    '',
    'PSI above this shows amber warning on drift dashboard',
    'threshold',
    false,
    'normal',
    ARRAY['ai','drift','psi']
);

SELECT _qtss_register_key(
    'ai.drift.psi_critical_threshold',
    'ai',
    'ai',
    'float',
    '0.25'::jsonb,
    '',
    'PSI above this triggers circuit breaker (if enabled)',
    'threshold',
    false,
    'normal',
    ARRAY['ai','drift','psi']
);

SELECT _qtss_register_key(
    'ai.drift.compute_interval_hours',
    'ai',
    'ai',
    'float',
    '6.0'::jsonb,
    '',
    'Hours between PSI recomputation',
    'interval',
    false,
    'normal',
    ARRAY['ai','drift','psi']
);
