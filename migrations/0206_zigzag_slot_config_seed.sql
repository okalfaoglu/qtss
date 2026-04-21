-- Seed the five configurable zigzag slots (L0..L4) with Fibonacci
-- defaults (3, 5, 8, 13, 21). Every worker and the GUI read these
-- via `qtss-config` — never hardcoded (CLAUDE.md #2).
--
-- Operators can tune per-slot length + display color at runtime from
-- the LuxAlgo chart UI; the update writes back here and both the
-- pivot writer and the GUI refetch on next cycle.

INSERT INTO system_config (module, config_key, value, description)
VALUES
    ('zigzag', 'slot_0', '{"length": 3,  "color": "#ef4444"}'::jsonb, 'Z1 — shortest zigzag (fastest pivots)'),
    ('zigzag', 'slot_1', '{"length": 5,  "color": "#3b82f6"}'::jsonb, 'Z2'),
    ('zigzag', 'slot_2', '{"length": 8,  "color": "#e5e7eb"}'::jsonb, 'Z3'),
    ('zigzag', 'slot_3', '{"length": 13, "color": "#f59e0b"}'::jsonb, 'Z4'),
    ('zigzag', 'slot_4', '{"length": 21, "color": "#a78bfa"}'::jsonb, 'Z5 — longest zigzag (structural)')
ON CONFLICT (module, config_key) DO NOTHING;
