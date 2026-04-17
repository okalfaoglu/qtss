-- Faz 9.8.1 — Setup selector thresholds.
--
-- These feed `qtss_risk::selector::SelectorConfig`. Tuned via Config
-- Editor at runtime (CLAUDE.md #2). The selector short-circuits on
-- the first rejection, so ordering also matters (see `with_defaults`
-- in the Rust crate — not configurable via DB yet).

SELECT _qtss_register_key(
    'selector.enabled', 'risk', 'selector',
    'bool', 'true'::jsonb, '',
    'Master switch for the Tier-2 setup selector.',
    'bool', true, 'high', ARRAY['risk','faz98','selector']
);

SELECT _qtss_register_key(
    'selector.min_risk_reward', 'risk', 'selector',
    'float', '1.5'::jsonb, '',
    'Minimum R:R ratio required for selection.',
    'number', false, 'normal', ARRAY['risk','faz98','selector']
);

SELECT _qtss_register_key(
    'selector.min_ai_score', 'risk', 'selector',
    'float', '0.55'::jsonb, '',
    'Minimum AI P(win) required for selection (0..1).',
    'number', false, 'normal', ARRAY['risk','faz98','selector']
);

SELECT _qtss_register_key(
    'selector.max_risk_pct', 'risk', 'selector',
    'float', '0.02'::jsonb, '',
    'Maximum allowed risk as fraction of equity (e.g. 0.02 = 2%).',
    'number', false, 'high', ARRAY['risk','faz98','selector']
);

SELECT _qtss_register_key(
    'selector.min_tier', 'risk', 'selector',
    'int', '6'::jsonb, '',
    'Minimum tier (1..10) required for selection.',
    'number', false, 'normal', ARRAY['risk','faz98','selector']
);

SELECT _qtss_register_key(
    'selector.max_open_positions_per_symbol', 'risk', 'selector',
    'int', '1'::jsonb, '',
    'Cap on concurrent live positions per (exchange, symbol).',
    'number', false, 'normal', ARRAY['risk','faz98','selector']
);
