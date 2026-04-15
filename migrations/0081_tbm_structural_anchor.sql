-- P22f — TBM structural anchor picker.
-- Replaces plain argmin/argmax over the last 50 bars with a pivot-based
-- ranker that scores candidates on (depth + rejection wick + volume
-- climax + liquidity sweep) and requires right-hand confirmation bars.
-- Previous code anchored labels to the currently forming bar far too
-- often — no wick, no confirmation, no reversal context. The new picker
-- mirrors the textbook Sweep→Rejection legs of reversal detection;
-- BoS/Retest confirmation comes in the P23 state machine.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('tbm', 'anchor.pivot_radius', '"3"',
   'P22f — bars on each side a candidate must dominate to count as a pivot extreme. 3 = standard fractal; raise to 5 for slower pivots on 1d+.'),
  ('tbm', 'anchor.min_right_bars', '"3"',
   'P22f — minimum completed bars AFTER a candidate before it is eligible as an anchor. Keeps the picker off the currently forming bar. 3 bars = typical Wyckoff right-side confirmation window.'),
  ('tbm', 'anchor.wick_min_ratio', '"0.25"',
   'P22f — candidate must have lower/upper wick ≥ 25% of its total range. Eliminates mid-trend bars without a rejection tail. Raise toward 0.4 for cleaner sweeps, lower toward 0.15 for noisier markets.'),
  ('tbm', 'anchor.vol_min_ratio', '"1.0"',
   'P22f — bar volume baseline vs 20-bar average for climax bonus. Below this ratio contributes 0; above, contributes linearly up to +1. 1.0 = neutral (average).')
ON CONFLICT (module, config_key) DO NOTHING;
