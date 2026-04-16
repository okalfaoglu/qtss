-- 0126_faz9_ml_explain_toggle.sql
--
-- Faz 9.3.4 — Independent kill-switch for the sidecar `/explain`
-- endpoint. Explain calls run booster.predict(..., pred_contrib=True)
-- which is noticeably slower than /score; operators need to be able to
-- drop the extra round-trip without disabling inference altogether.

SELECT _qtss_register_key(
    'inference.explain_enabled',
    'ai',
    'inference',
    'bool',
    'true'::jsonb,
    'QTSS_AI_INFERENCE_EXPLAIN_ENABLED',
    'Call the sidecar /explain endpoint in addition to /score to attach top-10 SHAP contributions on each prediction. Disable independently of inference.enabled if explain latency becomes a problem.',
    'bool',
    false,
    'normal',
    ARRAY['ai','inference','faz9','shap']
);
