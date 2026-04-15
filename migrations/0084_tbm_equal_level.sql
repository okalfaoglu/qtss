-- P22g — Equal lows / equal highs detection for TBM anchor picker.
-- Multiple prior pivot touches at roughly the same level = liquidity
-- pool (stops stacked there). A sweep that takes out a lone low is
-- weaker than one that takes out a level tested 2-3 times. The anchor
-- picker now counts prior pivot touches within `equal_level_tol` of
-- the candidate's price and awards a composite-score bonus when the
-- count meets `equal_level_min_touches`. Hard gate via
-- `equal_level_required` for pure double/triple-bottom mode.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('tbm', 'anchor.equal_level_tol', '"0.002"',
   'P22g — tolerance (fraction of price) within which a prior pivot counts as an equal-level touch. 0.002 = 0.2%. Tighten for cleaner double/triple bottoms/tops, loosen for wider liquidity pools.'),
  ('tbm', 'anchor.equal_level_min_touches', '"1"',
   'P22g — minimum number of prior pivot touches (NOT counting the candidate itself) at roughly the candidate''s price for the equal-level bonus to fire. 1 = classic double bottom/top, 2 = triple.'),
  ('tbm', 'anchor.equal_level_required', '"0"',
   'P22g — when 1, a candidate without the required equal-level touches is rejected outright (pure liquidity-pool mode). 0 = bonus only.')
ON CONFLICT (module, config_key) DO NOTHING;
