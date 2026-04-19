-- 0174_faz_9c_shadow_predictions.sql
--
-- Faz 9C — shadow/A-B inference audit.
--
-- qtss_models grew a `role` column in 0173 (active|shadow|archived);
-- the sidecar now scores all non-archived models in parallel, not
-- just the active one. To tell prediction rows apart for calibration
-- and A/B drift analytics we need a matching `role` column here.
--
-- Default 'active' so legacy rows inherit the old behavior — every
-- existing row was produced by the active model.
--
-- Idempotent.

ALTER TABLE qtss_ml_predictions
  ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'active'
    CHECK (role IN ('active','shadow'));

-- Role-aware indexes. Most dashboards filter by model_version first,
-- then partition by role — keep the composite tight.
CREATE INDEX IF NOT EXISTS idx_ml_predictions_role_ts
  ON qtss_ml_predictions(role, inference_ts DESC);
CREATE INDEX IF NOT EXISTS idx_ml_predictions_model_role
  ON qtss_ml_predictions(model_version, role);

COMMENT ON COLUMN qtss_ml_predictions.role IS
  'Faz 9C — active | shadow. Shadow rows are non-binding (setup gate ignores them) but feed calibration + A/B comparisons.';
