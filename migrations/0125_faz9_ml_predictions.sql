-- 0125_faz9_ml_predictions.sql
--
-- Faz 9.3.5 — Dedicated ML prediction telemetry table.
--
-- Why is this separate from `qtss_v2_setups.ai_score`?
--   (a) predictions can be requested even when *no* setup is ultimately
--       inserted (commission-gated, classic-gated, duplicate, etc.). We
--       still want model telemetry on those rows to measure AI vs
--       classic offline.
--   (b) a single setup may be re-scored in the future (online reload),
--       so a 1:1 denormalized column on the setup is insufficient.
--   (c) Model A/B, drift monitoring, latency/shap/feature_hash reports
--       need columns that have no business living on a setup row.
--
-- `qtss_v2_setups.ai_score` remains as a denormalized convenience for
-- the GUI's Setups list. This table is the source of truth.

CREATE TABLE IF NOT EXISTS qtss_ml_predictions (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    setup_id              UUID REFERENCES qtss_v2_setups(id)     ON DELETE CASCADE,
    detection_id          UUID REFERENCES qtss_v2_detections(id) ON DELETE SET NULL,
    exchange              TEXT NOT NULL,
    symbol                TEXT NOT NULL,
    timeframe             TEXT NOT NULL,
    model_name            TEXT NOT NULL,
    model_version         TEXT NOT NULL,
    model_sha             TEXT,
    feature_spec_version  TEXT NOT NULL,
    feature_hash          TEXT,
    score                 REAL NOT NULL,
    threshold             REAL NOT NULL,
    gate_enabled          BOOLEAN NOT NULL,
    decision              TEXT NOT NULL
                          CHECK (decision IN ('pass', 'block', 'shadow')),
    shap_top10            JSONB,
    latency_ms            INTEGER NOT NULL,
    sidecar_url           TEXT NOT NULL,
    request_id            UUID NOT NULL DEFAULT gen_random_uuid(),
    inference_ts          TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_ml_predictions_setup
    ON qtss_ml_predictions(setup_id)
    WHERE setup_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_ml_predictions_model_ts
    ON qtss_ml_predictions(model_version, inference_ts DESC);

CREATE INDEX IF NOT EXISTS idx_ml_predictions_symbol_ts
    ON qtss_ml_predictions(exchange, symbol, timeframe, inference_ts DESC);

CREATE INDEX IF NOT EXISTS idx_ml_predictions_decision_ts
    ON qtss_ml_predictions(decision, inference_ts DESC);

COMMENT ON TABLE  qtss_ml_predictions IS
    'One row per sidecar /score call. Lives independent of qtss_v2_setups because predictions happen even for rejected/unborn setups and we need latency/sha/feature_hash/shap telemetry for model drift + A/B analysis.';
COMMENT ON COLUMN qtss_ml_predictions.decision IS
    'shadow = gate off, logged only; pass/block = gate on and consulted.';
COMMENT ON COLUMN qtss_ml_predictions.feature_hash IS
    'SHA-256 hex of the canonical-JSON feature vector sent to the sidecar; empty/null if computation failed.';

-- Kill-switch for sidecar traffic when the gate is off.
SELECT _qtss_register_key(
    'inference.log_shadow_predictions',
    'ai',
    'inference',
    'bool',
    'true'::jsonb,
    'QTSS_AI_INFERENCE_LOG_SHADOW_PREDICTIONS',
    'When gate_enabled=false, still POST to sidecar and persist the prediction so we can measure AI vs classic offline. Setting false is a kill-switch for sidecar traffic.',
    'bool',
    false,
    'normal',
    ARRAY['ai','inference','faz9']
);
