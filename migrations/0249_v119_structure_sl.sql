-- Allocator v1.1.9 — structure-aware SL (ChatGPT teardown #4).
--
-- Old: sl = entry - atr × atr_sl_mult (flat volatility stop).
-- New: sl = entry - max(atr × atr_sl_mult, struct_dist × factor)
-- where struct_dist = distance to the nearest opposing-direction
-- swing pivot on the candidate (symbol, TF). Prevents "SL inside
-- the last swing low" — a classic noise-trap.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('allocator_v2', 'structure_sl_enabled',
     '{"enabled": true}'::jsonb,
     'When true, SL respects the nearest opposing pivot (swing low for longs, swing high for shorts) and is placed at struct_dist × structure_sl_factor away from entry if that is wider than the ATR-based stop.'),
    ('allocator_v2', 'structure_sl_factor',
     '{"value": 0.8}'::jsonb,
     'Proportion of the swing distance honoured. 0.8 keeps the SL just inside the structure to avoid exact-pivot sniping. Clamped to [0.1, 1.5] at load time.')
ON CONFLICT (module, config_key) DO NOTHING;
