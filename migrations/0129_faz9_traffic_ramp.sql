-- 0129_faz9_traffic_ramp.sql
--
-- Faz 9.4.4 — Traffic Ramp percentage.
--
-- Allows gradual ramp-up of AI gating: 0.0 = all shadow,
-- 1.0 = all gated. Random selection per-setup.

SELECT _qtss_register_key(
    'ai.inference.gate_pct',
    'ai',
    'ai',
    'float',
    '0.0'::jsonb,
    '',
    'Percentage of eligible setups that go through the AI gate (0.0 = all shadow, 1.0 = all gated). Random selection per-setup. Allows gradual ramp-up.',
    'ratio',
    false,
    'normal',
    ARRAY['ai','inference','gate','ramp']
);
