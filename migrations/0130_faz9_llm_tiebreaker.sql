-- 0130_faz9_llm_tiebreaker.sql
--
-- Faz 9.5 — LLM Tiebreaker.
--
-- When the LightGBM model scores a setup in the "uncertain zone"
-- (default 0.45–0.55), the system optionally consults an LLM for a
-- second opinion. The LLM sees the setup context and renders a
-- verdict: pass/block/abstain with confidence and reasoning text.
--
-- Soft-fail by design: if the LLM is unreachable or disabled, the
-- classic gate path continues unimpeded.

-- ─── config keys ─────────────────────────────────────────────────

SELECT _qtss_register_key(
    'llm.enabled',
    'ai',
    'llm',
    'bool',
    'false'::jsonb,
    '',
    'Enable the LLM tiebreaker for setups whose AI score falls in the uncertain zone.',
    'bool',
    false,
    'normal',
    ARRAY['ai','llm','faz9']
);

SELECT _qtss_register_key(
    'llm.provider',
    'ai',
    'llm',
    'string',
    '"claude"'::jsonb,
    '',
    'LLM provider: claude | gemini | ollama',
    'text',
    false,
    'normal',
    ARRAY['ai','llm','faz9']
);

SELECT _qtss_register_key(
    'llm.model',
    'ai',
    'llm',
    'string',
    '"claude-sonnet-4-20250514"'::jsonb,
    '',
    'Model identifier sent to the LLM provider API.',
    'text',
    false,
    'normal',
    ARRAY['ai','llm','faz9']
);

SELECT _qtss_register_key(
    'llm.api_key',
    'ai',
    'llm',
    'string',
    '""'::jsonb,
    '',
    'API key for the LLM provider (Anthropic/Google). Empty = disabled.',
    'secret',
    false,
    'normal',
    ARRAY['ai','llm','faz9']
);

SELECT _qtss_register_key(
    'llm.ollama_url',
    'ai',
    'llm',
    'string',
    '"http://127.0.0.1:11434"'::jsonb,
    '',
    'Base URL for the Ollama local LLM server.',
    'url',
    false,
    'normal',
    ARRAY['ai','llm','faz9']
);

SELECT _qtss_register_key(
    'llm.timeout_ms',
    'ai',
    'llm',
    'int',
    '10000'::jsonb,
    '',
    'Per-call timeout for the LLM tiebreaker request.',
    'milliseconds',
    false,
    'normal',
    ARRAY['ai','llm','faz9']
);

SELECT _qtss_register_key(
    'llm.uncertain_lo',
    'ai',
    'llm',
    'float',
    '0.45'::jsonb,
    '',
    'AI score lower bound of uncertain zone',
    'probability',
    false,
    'normal',
    ARRAY['ai','llm','faz9']
);

SELECT _qtss_register_key(
    'llm.uncertain_hi',
    'ai',
    'llm',
    'float',
    '0.55'::jsonb,
    '',
    'AI score upper bound of uncertain zone',
    'probability',
    false,
    'normal',
    ARRAY['ai','llm','faz9']
);

SELECT _qtss_register_key(
    'llm.max_tokens',
    'ai',
    'llm',
    'int',
    '256'::jsonb,
    '',
    'Maximum output tokens for the LLM tiebreaker response.',
    'count',
    false,
    'normal',
    ARRAY['ai','llm','faz9']
);

SELECT _qtss_register_key(
    'llm.prompt_version',
    'ai',
    'llm',
    'string',
    '"v1"'::jsonb,
    '',
    'Prompt template version for audit trail',
    'text',
    false,
    'normal',
    ARRAY['ai','llm','faz9']
);

-- ─── verdicts table ──────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS qtss_llm_verdicts (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    prediction_id   UUID NOT NULL REFERENCES qtss_ml_predictions(id) ON DELETE CASCADE,
    provider        TEXT NOT NULL,
    model           TEXT NOT NULL,
    prompt_version  TEXT NOT NULL,
    verdict         TEXT NOT NULL CHECK (verdict IN ('pass', 'block', 'abstain')),
    confidence      REAL,
    reasoning       TEXT,
    input_tokens    INTEGER,
    output_tokens   INTEGER,
    latency_ms      INTEGER NOT NULL,
    raw_response    JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_llm_verdicts_prediction_id
    ON qtss_llm_verdicts (prediction_id);

CREATE INDEX IF NOT EXISTS idx_llm_verdicts_verdict_created
    ON qtss_llm_verdicts (verdict, created_at DESC);

-- ─── widen reject_reason CHECK ───────────────────────────────────

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
        'ai_gate',
        'llm_block'
    ));
