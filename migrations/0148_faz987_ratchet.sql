-- Faz 9.8.7 — Ratchet policy config keys.

SELECT _qtss_register_key(
    'ratchet.enabled', 'risk', 'ratchet',
    'bool', 'true'::jsonb, '',
    'Master switch for the stop-loss ratchet.',
    'bool', true, 'high', ARRAY['risk','faz98','ratchet']
);

SELECT _qtss_register_key(
    'ratchet.breakeven_trigger_r', 'risk', 'ratchet',
    'float', '1.0'::jsonb, 'R',
    'R-multiple at which SL snaps to breakeven.',
    'number', false, 'normal', ARRAY['risk','faz98','ratchet']
);

SELECT _qtss_register_key(
    'ratchet.breakeven_offset_pct', 'risk', 'ratchet',
    'float', '0.0005'::jsonb, '',
    'Fractional offset above entry (below for shorts) to cover fees at breakeven.',
    'number', false, 'normal', ARRAY['risk','faz98','ratchet']
);

SELECT _qtss_register_key(
    'ratchet.trailing_atr_mult', 'risk', 'ratchet',
    'float', '2.0'::jsonb, 'atr',
    'Trailing-stop distance from the mark, in ATR units.',
    'number', false, 'normal', ARRAY['risk','faz98','ratchet']
);

SELECT _qtss_register_key(
    'ratchet.chandelier_atr_mult', 'risk', 'ratchet',
    'float', '3.0'::jsonb, 'atr',
    'Chandelier-exit distance from best_price, in ATR units.',
    'number', false, 'normal', ARRAY['risk','faz98','ratchet']
);

SELECT _qtss_register_key(
    'ratchet.chandelier_trigger_r', 'risk', 'ratchet',
    'float', '2.0'::jsonb, 'R',
    'R-multiple before chandelier policy activates.',
    'number', false, 'normal', ARRAY['risk','faz98','ratchet']
);
