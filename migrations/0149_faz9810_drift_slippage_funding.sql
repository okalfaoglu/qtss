-- Faz 9.8.10 — drift / slippage / funding guard config.

SELECT _qtss_register_key(
    'drift.max_drift_pct', 'risk', 'drift',
    'float', '0.015'::jsonb, '',
    'Critical price drift (fraction) between setup creation and execution.',
    'number', false, 'high', ARRAY['risk','faz98','drift']
);

SELECT _qtss_register_key(
    'drift.warn_drift_pct', 'risk', 'drift',
    'float', '0.005'::jsonb, '',
    'Warn-level price drift threshold.',
    'number', false, 'normal', ARRAY['risk','faz98','drift']
);

SELECT _qtss_register_key(
    'slippage.max_slippage_pct', 'risk', 'slippage',
    'float', '0.005'::jsonb, '',
    'Critical adverse-slippage threshold (fraction of expected price).',
    'number', false, 'high', ARRAY['risk','faz98','slippage']
);

SELECT _qtss_register_key(
    'slippage.warn_slippage_pct', 'risk', 'slippage',
    'float', '0.002'::jsonb, '',
    'Warn-level adverse-slippage threshold.',
    'number', false, 'normal', ARRAY['risk','faz98','slippage']
);

SELECT _qtss_register_key(
    'funding.max_adverse_rate', 'risk', 'funding',
    'float', '0.001'::jsonb, '',
    'Critical next-interval funding rate (adverse direction).',
    'number', false, 'high', ARRAY['risk','faz98','funding']
);

SELECT _qtss_register_key(
    'funding.warn_adverse_rate', 'risk', 'funding',
    'float', '0.0003'::jsonb, '',
    'Warn-level adverse funding rate.',
    'number', false, 'normal', ARRAY['risk','faz98','funding']
);
