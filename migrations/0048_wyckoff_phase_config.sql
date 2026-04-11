-- 0048: Wyckoff full phase config seed (Faz 10).

INSERT INTO system_config (module, config_key, value, description)
VALUES
  -- Phase A
  ('detector', 'wyckoff.sc_volume_multiplier',     '2.5',  'SC/BC volume must be >= Nx avg volume'),
  ('detector', 'wyckoff.sc_bar_width_multiplier',   '2.0',  'SC/BC bar range >= Nx ATR'),
  ('detector', 'wyckoff.st_max_volume_ratio',       '0.7',  'ST volume must be <= Nx SC volume'),
  ('detector', 'wyckoff.ar_min_retracement',        '0.3',  'AR must retrace >= N% of SC drop'),
  -- Phase B
  ('detector', 'wyckoff.ua_max_exceed_pct',         '0.03', 'UA may exceed AR by at most N%'),
  ('detector', 'wyckoff.stb_volume_decay_min',      '0.85', 'Each ST-B volume <= N * prev ST-B'),
  -- Phase C
  ('detector', 'wyckoff.shakeout_min_penetration',  '0.05', 'Shakeout penetration >= N% of range'),
  ('detector', 'wyckoff.shakeout_recovery_bars',    '3',    'Shakeout must recover within N bars'),
  -- Phase D
  ('detector', 'wyckoff.sos_min_volume_ratio',      '1.5',  'SOS volume >= Nx average'),
  ('detector', 'wyckoff.lps_max_retracement',       '0.5',  'LPS retracement <= N% of SOS move'),
  ('detector', 'wyckoff.lps_max_volume_ratio',      '0.5',  'LPS volume <= N% of SOS volume'),
  ('detector', 'wyckoff.creek_level_percentile',    '0.6',  'Creek = N% of range height'),
  -- Sloping
  ('detector', 'wyckoff.slope_threshold_deg',       '5.0',  'Range slope > Ndeg = sloping structure'),
  -- SOT
  ('detector', 'wyckoff.sot_thrust_decay_ratio',    '0.7',  'Each thrust <= N * prev thrust = SOT')
ON CONFLICT (module, config_key) DO NOTHING;
