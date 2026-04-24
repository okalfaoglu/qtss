-- Setup v1.1.1 — RR >= 2.0 + MTF opposing-direction gate config.
--
-- Post-mortem showed every armed setup landed with RR ≈ 1.00 because
-- atr_sl_mult and atr_tp_mult[0] were both 1.5. Even at 50% win rate
-- that's a break-even system net of commission; at 33% observed win
-- rate it's a guaranteed grind-down. We tighten SL to 1.0 × ATR and
-- stretch TP1 to 2.0 × ATR → RR = 2.0.
--
-- Separately, a same-symbol opposing-direction gate stops the
-- allocator from arming both BTCUSDT 1d LONG and BTCUSDT 15m SHORT
-- at the same time (observed live). The gate is "soft" — it just
-- skips the NEW setup when an opposite-direction armed setup exists
-- for the same symbol. Disables by setting to false.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('allocator_v2', 'atr_sl_mult',
     '{"value": 1.0}'::jsonb,
     'ATR multiple for stop-loss distance. Default 1.0 pairs with atr_tp_mult[0]=2.0 for RR≈2.0 on the first take-profit.'),
    ('allocator_v2', 'atr_tp_mult_0',
     '{"value": 2.0}'::jsonb,
     'ATR multiple for TP1 (partial-take level 1). RR = atr_tp_mult_0 / atr_sl_mult ≥ 2.0 ensures positive expectancy at win rate ≥ 33%.'),
    ('allocator_v2', 'atr_tp_mult_1',
     '{"value": 3.5}'::jsonb,
     'ATR multiple for TP2. Stretched from 3.0 so the ladder widens fairly.'),
    ('allocator_v2', 'atr_tp_mult_2',
     '{"value": 5.5}'::jsonb,
     'ATR multiple for TP3 (runner). Slight stretch from 5.0 for the tail trade.'),
    ('allocator_v2', 'mtf_opposing_gate_enabled',
     '{"enabled": true}'::jsonb,
     'When true, the allocator will not arm a new setup for (symbol) if any OTHER armed setup on the same symbol points in the opposite direction. Prevents self-hedging on multi-TF conflicts. Set to false to allow dual exposure (e.g. hedge-mode).')
ON CONFLICT (module, config_key) DO NOTHING;
