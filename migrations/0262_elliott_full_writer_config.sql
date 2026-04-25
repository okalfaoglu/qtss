-- FAZ 25.x — elliott_full writer activation.
--
-- Surfaces the dormant ElliottDetectorSet (leading + ending diagonal,
-- regular/expanded/running flat, W1/W3/W5 extended impulse, truncated
-- fifth, W-X-Y combination). User: "itki ve abc dışında diğer elliott
-- dalgalarını kod tarama yapmıyor mu?" — yes, the code existed, this
-- migration finally turns it on.
--
-- Engine writer registration: crates/qtss-engine/src/lib.rs (after
-- ElliottWriter so the LuxAlgo Pine port keeps owning motive/abc/
-- triangle subkinds; elliott_full skips those by design).
--
-- Output: detections.pattern_family = 'elliott_full', subkind carries
-- the detector's own label (e.g. flat_expanded_bear,
-- leading_diagonal_bull, combination_wxy_zigzag_zigzag_bear).

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('elliott_full', 'enabled',
     '{"enabled": true}'::jsonb,
     'Master switch for the elliott_full writer.'),
    ('elliott_full', 'bars_per_tick',
     '{"bars": 2000}'::jsonb,
     'Bars per symbol fetched into memory per writer tick.'),
    ('elliott_full', 'pivots_per_slot',
     '{"count": 500}'::jsonb,
     'Maximum pivots loaded for the configured pivot_level.'),
    ('elliott_full', 'pivot_level',
     '"L1"'::jsonb,
     'PivotTree level fed to the dormant detectors. L0 = densest, L4 = sparsest. Default L1 matches the SmcWriter so both crates see the same pivot stream.'),
    ('elliott_full', 'min_structural_score',
     '{"value": 0.45}'::jsonb,
     'Detector-side score floor (0..1). Validator may raise the bar later.'),
    ('elliott_full', 'toggles.leading_diagonal',
     '{"enabled": true}'::jsonb,
     'Leading diagonal scanner (W1 or W5 in motive context).'),
    ('elliott_full', 'toggles.ending_diagonal',
     '{"enabled": true}'::jsonb,
     'Ending diagonal scanner (terminal W5 wedge).'),
    ('elliott_full', 'toggles.flat',
     '{"enabled": true}'::jsonb,
     'Flat correction scanner — emits regular/expanded/running variants in subkind.'),
    ('elliott_full', 'toggles.extended_impulse',
     '{"enabled": true}'::jsonb,
     'Extended impulse scanner — flags which sub-wave (W1/W3/W5) is the extension.'),
    ('elliott_full', 'toggles.truncated_fifth',
     '{"enabled": true}'::jsonb,
     'Truncated fifth scanner — W5 fails to exceed W3.'),
    ('elliott_full', 'toggles.combination',
     '{"enabled": true}'::jsonb,
     'W-X-Y combination scanner — complex corrective with two simple sub-corrections joined by an X wave.')
ON CONFLICT (module, config_key) DO UPDATE
   SET value = EXCLUDED.value,
       description = EXCLUDED.description,
       updated_at = now();
