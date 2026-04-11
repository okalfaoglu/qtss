-- 0046: Config seed for range sub-detectors (FVG, Order Block, Liquidity Pool, Equal Levels).

INSERT INTO system_config (module, config_key, value, description)
VALUES
  ('detection', 'range_sub.enabled',                'true',  'Enable FVG / Order Block / Liquidity Pool / Equal Levels detectors'),

  -- FVG
  ('detection', 'range.fvg.enabled',                'true',  'Enable Fair Value Gap detection'),
  ('detection', 'range.fvg.min_gap_atr_frac',       '0.3',   'Minimum gap size as ATR fraction'),
  ('detection', 'range.fvg.scan_lookback',           '50',    'Bars to scan for FVGs'),
  ('detection', 'range.fvg.unfilled_only',           'true',  'Only report unfilled FVGs'),
  ('detection', 'range.fvg.volume_spike_mult',       '1.2',   'Volume spike multiplier for quality'),

  -- Order Block
  ('detection', 'range.order_block.enabled',         'true',  'Enable Order Block detection'),
  ('detection', 'range.order_block.impulse_atr_mult','1.5',   'Min impulse size as ATR multiple'),
  ('detection', 'range.order_block.impulse_candles',  '3',    'Number of impulse candles to measure'),
  ('detection', 'range.order_block.unmitigated_only', 'true', 'Only report unmitigated OBs'),

  -- Liquidity Pool
  ('detection', 'range.liquidity_pool.enabled',      'true',  'Enable Liquidity Pool detection'),
  ('detection', 'range.liquidity_pool.cluster_atr_mult','0.3','ATR tolerance for clustering pivots'),
  ('detection', 'range.liquidity_pool.min_touches',   '2',    'Minimum pivot touches for a pool'),

  -- Equal Levels
  ('detection', 'range.equal_levels.enabled',        'true',  'Enable Equal Highs/Lows detection'),
  ('detection', 'range.equal_levels.equal_tolerance_atr','0.15','Max price diff (ATR frac) for equal'),
  ('detection', 'range.equal_levels.min_bar_distance', '5',   'Min bar distance between equal pivots')
ON CONFLICT (module, config_key) DO NOTHING;
