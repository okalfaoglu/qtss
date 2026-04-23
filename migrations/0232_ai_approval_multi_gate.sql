-- Multi-gate AI approval (Faz 13C).
--
-- Extends `ai_approval_requests` with per-gate score breakdown,
-- rejection reason, and auto-approved flag. Also seeds the
-- per-gate thresholds the evaluator reads at run-time.
--
-- Philosophy: every setup goes through six gates — confidence,
-- meta-label, regime fit, confluence, risk budget, event blackout.
-- Any gate failing moves the request to `rejected` with a machine-
-- readable reason the operator / Telegram card can display. When
-- every gate passes AND confidence >= auto_approve_threshold, the
-- setup is auto-approved; otherwise it lands in `pending` for a
-- human to flip.

ALTER TABLE ai_approval_requests
    ADD COLUMN IF NOT EXISTS gate_scores      JSONB,
    ADD COLUMN IF NOT EXISTS rejection_reason TEXT,
    ADD COLUMN IF NOT EXISTS auto_approved    BOOLEAN NOT NULL DEFAULT false;

CREATE INDEX IF NOT EXISTS idx_ai_approval_rejection_reason
    ON ai_approval_requests (rejection_reason)
    WHERE rejection_reason IS NOT NULL;

COMMENT ON COLUMN ai_approval_requests.gate_scores IS
    'Per-gate {gate_name: {score, threshold, passed}} breakdown from the multi-gate evaluator. Exposed to Telegram card + GUI tooltip.';
COMMENT ON COLUMN ai_approval_requests.rejection_reason IS
    'Machine-readable tag for the first gate that failed (confidence_below / meta_label_below / regime_unsupported / confluence_below / risk_budget_exhausted / event_blackout).';
COMMENT ON COLUMN ai_approval_requests.auto_approved IS
    'True when every gate passed AND confidence >= auto_approve_threshold. False for human-reviewed approvals + rejections.';

-- Multi-gate thresholds.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('ai_approval', 'auto_approve_threshold', '{"value": 0.75}'::jsonb,
     'Minimum confidence for auto-approval. Below this, the setup lands in `pending` for human review even when all gates pass.'),
    ('ai_approval', 'gates.min_confidence', '{"value": 0.65}'::jsonb,
     'Gate 1 — raw detector confidence floor.'),
    ('ai_approval', 'gates.min_meta_label', '{"value": 0.55}'::jsonb,
     'Gate 2 — ML meta-label floor. Bypass when no model is loaded (returns 1.0).'),
    ('ai_approval', 'gates.min_confluence', '{"value": 0.60}'::jsonb,
     'Gate 3 — confluence-engine confidence floor (0..1 scale from ConfluenceSnapshot).'),
    ('ai_approval', 'gates.regime_blacklist', '{"regimes": ["choppy", "uncertain"]}'::jsonb,
     'Gate 4 — regimes in which every setup is rejected regardless of other scores.'),
    ('ai_approval', 'gates.max_daily_rejected_per_symbol', '{"value": 10}'::jsonb,
     'Gate 5 — per-symbol daily rejection cap. After this, the setup is auto-rejected with `risk_budget_exhausted`.'),
    ('ai_approval', 'gates.event_blackout_minutes', '{"value": 30}'::jsonb,
     'Gate 6 — minutes around a macro event during which every setup is blocked.')
ON CONFLICT (module, config_key) DO NOTHING;
