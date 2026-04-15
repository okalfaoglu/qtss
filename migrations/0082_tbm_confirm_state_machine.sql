-- P23a+b — TBM confirmation state machine (forming → confirmed | invalidated)
-- + P22f sweep_required toggle (pure-Wyckoff anchor mode).
--
-- forming rows now auto-promote to `confirmed` only after:
--   a) BoS: a close breaks the structural level on the opposite side
--      of the anchor (pre-anchor swing high for bottom, swing low for
--      top) within `window_bars` bars, AND
--   b) Follow-through: a close at least `followthrough_atr_mult × ATR(14)`
--      in the reversal direction within `followthrough_bars` of the BoS.
-- If the confirm window elapses without a BoS, the row is invalidated
-- (timeout). Without this, bottom/top_setup rows would sit in `forming`
-- indefinitely with no mechanism to graduate or die on their own.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('tbm', 'confirm.bos_required', '"1"',
   'P23 — master flag for BoS-gated confirmation. 1 = require break-of-structure + follow-through before a TBM setup graduates from forming to confirmed; 0 = legacy validator-only path.'),
  ('tbm', 'confirm.window_bars', '"8"',
   'P23 — bars to wait after the anchor for a BoS close to print. Expired without BoS → invalidated(timeout). 8 bars ≈ 8h on H1, ~1.3 days on 4h.'),
  ('tbm', 'confirm.followthrough_atr_mult', '"1.0"',
   'P23 — follow-through closing move measured in ATR(14) units above (bottom) / below (top) the anchor. 1.0 = one ATR; raise for stricter confirmation, lower for noisy markets.'),
  ('tbm', 'confirm.followthrough_bars', '"3"',
   'P23 — bars after the BoS bar within which the follow-through close must appear. 3 is a reasonable default for most TFs.'),
  ('tbm', 'anchor.sweep_required', '"0"',
   'P22f — when 1, a candidate anchor MUST take out a prior window extreme (liquidity sweep / fake breakdown / breakout) to qualify. 0 = sweep contributes as a weighted bonus only, so V-bottom reversals without a sweep still anchor. Toggle to 1 for pure-Wyckoff mode.')
ON CONFLICT (module, config_key) DO NOTHING;
