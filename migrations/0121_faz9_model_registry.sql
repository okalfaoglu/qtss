-- 0121_faz9_model_registry.sql
--
-- Faz 9.3 — Model registry.
--
-- One row per trained LightGBM artifact. The Rust inference hook
-- (later in 9.3.3) picks the row with `active=true` for a given
-- `model_family` and loads `artifact_path` at startup.
--
-- `metrics_json` stores cv auc / logloss / calibration info so the
-- GUI can chart training runs over time. `feature_spec_version`
-- pins which `qtss_features_snapshot` rows the model was trained on
-- — bump the spec → retrain → bump model_version.

CREATE TABLE IF NOT EXISTS qtss_models (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    model_family         TEXT NOT NULL,               -- 'setup_meta' | 'signal_rank' | ...
    model_version        TEXT NOT NULL,               -- '2026.04.16-01' or semver
    feature_spec_version INTEGER NOT NULL,
    algorithm            TEXT NOT NULL DEFAULT 'lightgbm',
    task                 TEXT NOT NULL,               -- 'binary' | 'multiclass' | 'regression'
    n_train              BIGINT NOT NULL,
    n_valid              BIGINT NOT NULL,
    metrics_json         JSONB NOT NULL DEFAULT '{}'::jsonb,
    params_json          JSONB NOT NULL DEFAULT '{}'::jsonb,
    feature_names        TEXT[] NOT NULL DEFAULT ARRAY[]::text[],
    artifact_path        TEXT NOT NULL,               -- absolute path on disk
    artifact_sha256      TEXT,
    trained_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    trained_by           TEXT,                        -- operator / 'cron' / etc.
    notes                TEXT,
    active               BOOLEAN NOT NULL DEFAULT false,
    CONSTRAINT qtss_models_family_version_uq UNIQUE (model_family, model_version)
);

CREATE INDEX IF NOT EXISTS idx_qtss_models_family_active
    ON qtss_models (model_family, active, trained_at DESC);

COMMENT ON TABLE qtss_models IS
  'Faz 9.3 — trained model registry; `active=true` row is served by the inference hook.';

-- Only one active row per family.
CREATE UNIQUE INDEX IF NOT EXISTS qtss_models_one_active_per_family
    ON qtss_models (model_family) WHERE active;

-- ---------------------------------------------------------------------------
-- Config defaults (CLAUDE.md #2: nothing hardcoded).
-- ---------------------------------------------------------------------------

SELECT _qtss_register_key(
    'trainer.model_family',
    'ai',
    'trainer',
    'string',
    '"setup_meta"'::jsonb,
    '',
    'Which model family the Faz 9.3 trainer targets.',
    'text', false, 'normal',
    ARRAY['ai','trainer','faz9']
);

SELECT _qtss_register_key(
    'trainer.artifact_dir',
    'ai',
    'trainer',
    'string',
    '"/app/qtss/artifacts/models"'::jsonb,
    '',
    'Directory where LightGBM .txt artifacts are written.',
    'text', false, 'normal',
    ARRAY['ai','trainer','faz9']
);

SELECT _qtss_register_key(
    'trainer.min_rows',
    'ai',
    'trainer',
    'int',
    '500'::jsonb,
    '',
    'Abort training below this many closed+labeled setups.',
    'count', false, 'normal',
    ARRAY['ai','trainer','faz9']
);

SELECT _qtss_register_key(
    'trainer.validation_fraction',
    'ai',
    'trainer',
    'float',
    '0.2'::jsonb,
    '',
    'Fraction of rows held out for validation metrics (time-ordered split).',
    'ratio', false, 'normal',
    ARRAY['ai','trainer','faz9']
);

SELECT _qtss_register_key(
    'trainer.lgbm_params',
    'ai',
    'trainer',
    'object',
    '{"objective":"binary","metric":["auc","binary_logloss"],"learning_rate":0.05,"num_leaves":31,"min_data_in_leaf":20,"feature_fraction":0.9,"bagging_fraction":0.8,"bagging_freq":5,"verbose":-1}'::jsonb,
    '',
    'LightGBM hyperparameters; override without redeploy via Config Editor.',
    'json', false, 'normal',
    ARRAY['ai','trainer','faz9']
);

SELECT _qtss_register_key(
    'trainer.num_boost_round',
    'ai',
    'trainer',
    'int',
    '500'::jsonb,
    '',
    'Maximum boosting rounds; early stopping applies on validation.',
    'count', false, 'normal',
    ARRAY['ai','trainer','faz9']
);
