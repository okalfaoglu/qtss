-- Faz 9.8.2 — Risk allocator config keys.
--
-- Feed `qtss_risk::allocator::AllocatorConfig`. Tunable at runtime
-- via the Config Editor (CLAUDE.md #2). The allocator walks its gate
-- dispatch table (equity floor → drawdown → exposure → commission)
-- and only on pass runs sizing.

SELECT _qtss_register_key(
    'allocator.max_session_drawdown', 'risk', 'allocator',
    'float', '0.10'::jsonb, '',
    'Session drawdown cap — allocator blocks new entries once current drawdown >= this fraction of peak equity.',
    'number', false, 'high', ARRAY['risk','faz98','allocator']
);

SELECT _qtss_register_key(
    'allocator.min_equity', 'risk', 'allocator',
    'float', '100'::jsonb, 'quote',
    'Equity floor — no new entries below this absolute equity.',
    'number', false, 'high', ARRAY['risk','faz98','allocator']
);

SELECT _qtss_register_key(
    'allocator.max_gross_exposure', 'risk', 'allocator',
    'float', '3.0'::jsonb, 'x',
    'Gross-notional-to-equity cap across open positions. Sizing trims on approach; gate rejects on breach.',
    'number', false, 'high', ARRAY['risk','faz98','allocator']
);

SELECT _qtss_register_key(
    'allocator.min_edge_ratio', 'risk', 'allocator',
    'float', '1.5'::jsonb, '',
    'Commission gate: gross_pct / (fee_pct + slippage_pct) must exceed this ratio.',
    'number', false, 'normal', ARRAY['risk','faz98','allocator']
);

SELECT _qtss_register_key(
    'allocator.slippage_bps', 'risk', 'allocator',
    'float', '10'::jsonb, 'bps',
    'Slippage buffer added to commission for the edge calculation (basis points).',
    'number', false, 'normal', ARRAY['risk','faz98','allocator']
);
