-- 0117_faz9_confluence_gate.sql
--
-- Faz 9.1 — Classic Confluence Gate.
--
--   * Extend qtss_v2_setup_rejections.reject_reason CHECK constraint
--     with the new gate veto slugs.
--   * Seed default gate config under module='setup' / group='confluence_gate'.

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
        'gate_no_direction'
    ));

-- Gate config — CLAUDE.md #2 (no hardcoded thresholds).
SELECT _qtss_register_key(
    'confluence_gate.enabled','setup','confluence_gate','bool',
    'true'::jsonb, '',
    'Faz 9.1 classic confluence gate master switch. When off, legacy arm_guven check remains the only barrier.',
    'bool', false, 'normal', ARRAY['setup','confluence_gate']);

SELECT _qtss_register_key(
    'confluence_gate.min_score','setup','confluence_gate','float',
    '0.55'::jsonb, '',
    'Minimum weighted guven required to approve a setup (Layer 3).',
    'number', false, 'normal', ARRAY['setup','confluence_gate']);

SELECT _qtss_register_key(
    'confluence_gate.min_direction_votes','setup','confluence_gate','int',
    '2'::jsonb, '',
    'Min structural family votes (elliott+wyckoff+classical) for direction consensus (Layer 2).',
    'number', false, 'normal', ARRAY['setup','confluence_gate']);

SELECT _qtss_register_key(
    'confluence_gate.reject_on_regimes','setup','confluence_gate','array',
    '[]'::jsonb, '',
    'Regime labels that categorically reject a candidate (JSON array of strings, case-insensitive).',
    'json', false, 'normal', ARRAY['setup','confluence_gate']);

SELECT _qtss_register_key(
    'confluence_gate.kill_switch_on','setup','confluence_gate','bool',
    'false'::jsonb, '',
    'Global kill switch — rejects every new setup (Layer 1 veto).',
    'bool', false, 'normal', ARRAY['setup','confluence_gate']);

SELECT _qtss_register_key(
    'confluence_gate.news_blackout_on','setup','confluence_gate','bool',
    'false'::jsonb, '',
    'News blackout veto (Layer 1). Wire the scheduler to flip this during high-impact releases.',
    'bool', false, 'normal', ARRAY['setup','confluence_gate']);
