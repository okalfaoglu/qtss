-- 0123_faz9_inference_hook.sql
--
-- Faz 9.3.3 — Rust-side inference hook.
--
-- At setup-open time, the D/T/Q loop calls the `qtss-trainer` Python
-- sidecar (FastAPI) which loads the active LightGBM booster once in
-- memory and returns a probability. The score is persisted on the
-- setup row for downstream analytics (and later, a gate).
--
-- Shadow-first deployment: `ai.inference.gate_enabled=false` by default,
-- so the hook only observes + records. Operators flip the gate on once
-- score distribution looks sane in the Training Set / AI Decisions panel.

ALTER TABLE qtss_v2_setups
    ADD COLUMN IF NOT EXISTS ai_score REAL;

-- Widen reject_reason CHECK so the new ai_gate rejection can be recorded.
ALTER TABLE qtss_v2_setup_rejections
    DROP CONSTRAINT IF EXISTS qtss_v2_setup_rejections_reject_reason_check;

ALTER TABLE qtss_v2_setup_rejections
    ADD CONSTRAINT qtss_v2_setup_rejections_reject_reason_check
    CHECK (reject_reason IN (
        'total_risk_cap',
        'max_concurrent',
        'correlation_cap',
        'commission_gate',
        'gate_kill_switch',
        'gate_stale_data',
        'gate_news_blackout',
        'gate_regime_opposite',
        'gate_direction_consensus',
        'gate_below_min_score',
        'gate_no_direction',
        'ai_gate'
    ));

COMMENT ON COLUMN qtss_v2_setups.ai_score IS
    'LightGBM P(win) at setup-open time, 0..1. NULL if the inference sidecar was disabled/unreachable/errored. Shadow-only until ai.inference.gate_enabled flips true.';

CREATE INDEX IF NOT EXISTS idx_v2_setups_ai_score
    ON qtss_v2_setups (ai_score DESC NULLS LAST);

SELECT _qtss_register_key(
    'inference.enabled',
    'ai',
    'inference',
    'bool',
    'true'::jsonb,
    'QTSS_AI_INFERENCE_ENABLED',
    'Call the inference sidecar at setup-open time. When false, setups insert with ai_score=NULL.',
    'bool',
    false,
    'normal',
    ARRAY['ai','inference','faz9']
);

SELECT _qtss_register_key(
    'inference.sidecar_url',
    'ai',
    'inference',
    'string',
    '"http://127.0.0.1:8790"'::jsonb,
    'QTSS_AI_INFERENCE_SIDECAR_URL',
    'Base URL of the qtss-trainer inference sidecar (FastAPI). POST /score is called with the feature dict.',
    'url',
    false,
    'normal',
    ARRAY['ai','inference','faz9']
);

SELECT _qtss_register_key(
    'inference.timeout_ms',
    'ai',
    'inference',
    'int',
    '300'::jsonb,
    'QTSS_AI_INFERENCE_TIMEOUT_MS',
    'Per-call timeout for the inference sidecar. Setup opens do not block on a slow sidecar — on timeout ai_score stays NULL.',
    'milliseconds',
    false,
    'normal',
    ARRAY['ai','inference','faz9']
);

SELECT _qtss_register_key(
    'inference.gate_enabled',
    'ai',
    'inference',
    'bool',
    'false'::jsonb,
    'QTSS_AI_INFERENCE_GATE_ENABLED',
    'Reject setups whose ai_score falls below min_score. OFF by default so the hook ships in shadow mode.',
    'bool',
    false,
    'normal',
    ARRAY['ai','inference','faz9']
);

SELECT _qtss_register_key(
    'inference.min_score',
    'ai',
    'inference',
    'float',
    '0.55'::jsonb,
    '',
    'Minimum P(win) a setup must clear when gate_enabled is true.',
    'probability',
    false,
    'normal',
    ARRAY['ai','inference','faz9']
);
