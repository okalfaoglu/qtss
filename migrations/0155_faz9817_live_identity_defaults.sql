-- Faz 9.8.17 — live-mode identity defaults.
--
-- The execution_bridge's live adapter needs a default (org_id, user_id)
-- pair so it can look up exchange_accounts credentials when a setup
-- arrives without an explicit owner. On single-tenant dev deploys this
-- is just "the operator's own row". Kept separate from the dry keys so
-- operators can point live at a different user (e.g. a sub-account)
-- without touching the paper flow.
--
-- Master safety gate `execution.live.enabled` (migration 0150) stays
-- false by default; flipping it on *with* these keys populated is what
-- actually sends orders to the broker.

SELECT _qtss_register_key(
    'execution.live.default_org_id', 'execution', 'live',
    'string', '""'::jsonb, '',
    'Org UUID used by the live broker adapter to load exchange_accounts credentials.',
    'text', false, 'normal', ARRAY['execution','faz9817','live']
);

SELECT _qtss_register_key(
    'execution.live.default_user_id', 'execution', 'live',
    'string', '""'::jsonb, '',
    'User UUID used by the live broker adapter to load exchange_accounts credentials.',
    'text', false, 'normal', ARRAY['execution','faz9817','live']
);
