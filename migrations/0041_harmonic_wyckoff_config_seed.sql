-- 0041: Seed system_config with harmonic / wyckoff / range detector tunables.
-- Default pivot_level lowered to L0 so both detectors receive enough
-- pivots from the standard 500-bar window.  Operators can bump to L1
-- from the Config Editor once they've verified sufficient L1 density.

INSERT INTO system_config (module, config_key, value, description)
VALUES
  -- Harmonic
  ('detection', 'harmonic.pivot_level',         '"L0"',   'Pivot level for harmonic XABCD detector (L0/L1/L2/L3)'),
  ('detection', 'harmonic.min_structural_score', '0.40',  'Minimum structural score (0..1) for harmonic detections'),
  ('detection', 'harmonic.global_slack',         '0.05',  'Extra tolerance around canonical ratio ranges (0..0.5)'),
  -- Wyckoff
  ('detection', 'wyckoff.pivot_level',          '"L0"',   'Pivot level for Wyckoff event detector (L0/L1/L2/L3)'),
  ('detection', 'wyckoff.min_range_pivots',      '5',     'Minimum pivots in range box for Wyckoff detection'),
  ('detection', 'wyckoff.min_structural_score',  '0.40',  'Minimum structural score (0..1) for Wyckoff detections'),
  ('detection', 'wyckoff.range_edge_tolerance',  '0.04',  'Max deviation between range highs/lows as fraction of mid'),
  ('detection', 'wyckoff.climax_volume_mult',    '1.8',   'Volume multiplier for climactic event identification'),
  ('detection', 'wyckoff.min_penetration',       '0.02',  'Min false-break penetration as fraction of range height'),
  ('detection', 'wyckoff.max_penetration',       '0.30',  'Max penetration before treated as true breakout'),
  -- Range
  ('detection', 'range.enabled',                 'true',  'Enable/disable range (FVG/trading-range) detector')
ON CONFLICT (module, config_key) DO NOTHING;
