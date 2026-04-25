-- FAZ 25.1 — Elliott rule tolerance + per-anchor Fib scoring config.
--
-- User raised: "Elliott hesaplarında hata payı çarpanımız var mı?
-- Bazen kurallara birebir uymasa da formasyonlar kabul görür."
--
-- Yes — every Elliott practitioner allows ~%5 wiggle on Fibonacci
-- bands because pivot prints in real markets are noisy. We expose
-- the tolerance as a config knob so an operator can tighten it for
-- aggressive setups or loosen for choppy markets without recompiling.
--
-- Fields:
--   fib_tolerance_pct: how far OUTSIDE a band a value can sit and
--     still pass (0.05 = ±5%). Applied symmetrically to upper and
--     lower edges.
--   anchor_score_enabled: when true, write_early stamps a per-anchor
--     `fib_score` (0-1, ideal-Fib proximity) into the anchors JSON
--     so the chart can render dereceli (graded) confidence instead of
--     binary solid/dotted.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('elliott_early', 'fib_tolerance_pct',
     '{"value": 0.05}'::jsonb,
     'Fractional widening applied symmetrically to every Elliott Fib band (A retracement 0.236..0.886, B retracement 0.236..0.786, C extension 0.618..2.618). 0.05 = ±5% — accepts a 0.224 retrace where the strict ceiling is 0.236. Set 0.0 for textbook strict.'),
    ('elliott_early', 'invalidation_tol_pct',
     '{"value": 0.005}'::jsonb,
     'Price tolerance for the W5-break invalidation rule (0.5%). Wick noise inside this band does NOT invalidate a motive.'),
    ('elliott_early', 'anchor_score_enabled',
     '{"enabled": true}'::jsonb,
     'When true, every ABC anchor gets a fib_score field (0-1) tracking how close its retracement / extension lands to the canonical Fibonacci targets {0.382, 0.5, 0.618, 1.0, 1.618, 2.618}. The frontend uses this for graded line opacity instead of pure solid/dotted.')
ON CONFLICT (module, config_key) DO NOTHING;
