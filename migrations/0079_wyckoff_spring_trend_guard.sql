-- P20 — Wyckoff Spring/UTAD trend filter + tightened established-range
-- thresholds. Previous defaults (min_edge_tests=2, min_range_age_bars=10)
-- were too permissive: in a clean uptrend the detector labeled every
-- minor pullback as a Spring because a sliding 2-low "test" set always
-- satisfied the guard. New defaults raise the bar and introduce a slope
-- check that outright rejects Spring/UTAD when the edge pivot series
-- is itself trending (see `same_kind_slope_frac` in events.rs).

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('detector', 'wyckoff.manipulation_max_edge_slope', '"0.004"',
   'P20 — reject Spring/UTAD when same-kind pivot slope exceeds this fraction-per-pivot (0.004 = 0.4%/pivot). Kills false positives in trending markets.')
ON CONFLICT (module, config_key) DO NOTHING;

-- Raise the two existing thresholds to their new defaults. Only update
-- rows that still hold the old values so operator tuning is preserved.
UPDATE system_config
   SET value = '"3"'
 WHERE module = 'detector'
   AND config_key = 'wyckoff.manipulation_min_edge_tests'
   AND value = '"2"';

UPDATE system_config
   SET value = '"20"'
 WHERE module = 'detector'
   AND config_key = 'wyckoff.manipulation_min_range_age_bars'
   AND value = '"10"';
