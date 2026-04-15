-- P23c — TBM retest detection for confirmed bottom/top setups.
-- After BoS + follow-through graduate a setup to `confirmed`, the
-- retest scanner looks for the textbook pullback: for a bottom, the
-- first bar after BoS whose low comes within `retest_proximity_atr ×
-- ATR(14)` of the broken pre-anchor swing high AND closes back above
-- it (HL, the classic "resistance-turned-support"). Mirrored for tops.
-- The retest bar is the textbook safest entry — GUI draws this as a
-- distinct marker so operators can distinguish "setup confirmed" vs
-- "entry-ready".

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('tbm', 'confirm.retest_max_age_bars', '"12"',
   'P23c — maximum bars after the BoS bar within which a retest is still accepted. Past this point we assume the trend moved on without a textbook retest. 12 is reasonable for most TFs; shorten on LTF scalping setups.'),
  ('tbm', 'confirm.retest_proximity_atr', '"0.5"',
   'P23c — how close (in ATR(14) multiples) the pullback wick must come to the broken structural level to count as a real retest. 0.5 = within half an ATR. Raise for more forgiving retests, lower for strict "touch" requirements.')
ON CONFLICT (module, config_key) DO NOTHING;
