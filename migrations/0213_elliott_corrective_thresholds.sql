-- Flat + Triangle corrective sub-type thresholds.
--
-- These replicate the constants in `crates/qtss-elliott/src/flat.rs`
-- and `crates/qtss-elliott/src/triangle.rs` so operators can tune them
-- live via the Config Editor without a redeploy (CLAUDE.md #2).
--
-- Note: the pine_port_corrective module currently reads compiled-in
-- constants; hooking it up to these keys is a follow-up (minor loader
-- change in v2_elliott.rs analogous to `load_min_prominence_pct`).
-- Seeding them now avoids one extra migration later.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('elliott_corrective', 'flat_min_b_ratio',
        '{"ratio": 0.90}'::jsonb,
        'Minimum |B|/|A| for an ABC to classify as a flat. Below this it stays a zigzag. Default 0.90 matches Frost & Prechter.'),
    ('elliott_corrective', 'flat_running',
        '{"min_b": 1.05, "min_c": 0.0, "max_c": 1.00}'::jsonb,
        'Running flat: B overshoots A (>=1.05) and C fails to reach A end (<=1.00).'),
    ('elliott_corrective', 'flat_expanded',
        '{"min_b": 1.05, "min_c": 1.05, "max_c": 2.00}'::jsonb,
        'Expanded flat: B overshoots A and C extends beyond A. Most common in liquid markets.'),
    ('elliott_corrective', 'flat_regular',
        '{"min_b": 0.90, "min_c": 0.85, "max_c": 1.15}'::jsonb,
        'Regular flat: B ~= 100%% of A, C ~= 100%% of A.'),
    ('elliott_corrective', 'triangle_barrier_flat_tol',
        '{"tol": 0.05}'::jsonb,
        'Fractional tolerance for a triangle trendline to count as "horizontal" (barrier type). 0.05 = 5%%.'),
    ('elliott_corrective', 'enabled',
        '{"enabled": true}'::jsonb,
        'Toggle Flat + Triangle refinement in the Pine port. Set false to emit only plain ABC/zigzag.')
ON CONFLICT (module, config_key) DO NOTHING;
