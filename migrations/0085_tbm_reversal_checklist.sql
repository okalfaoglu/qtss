-- P23d — Reversal Confidence Checklist (0–5) for TBM setups.
-- Each TBM row now carries a 5-point scorecard in raw_meta under
-- `reversal_checklist.{flags,score,tier}`. Flags map 1-for-1 onto the
-- textbook reversal framework (Sweep → Rejection → BoS → Retest +
-- Volume climax). Tier ladder: 5=elite, 4=strong, 3=ok, 2=weak, ≤1=
-- filtered. Downstream filters (GUI default view, alerting, risk
-- allocator) key off `tier` so the scorecard is the single source of
-- truth for "how textbook is this reversal".
--
-- Seeds two config keys:
--   * min_tier — GUI/alert filter floor (alphanumeric tier name).
--   * min_score — numeric floor for the same filter (pick whichever
--     is stricter when both are consulted).
-- Flag computation lives in the detector; no extra knobs required for
-- the scoring itself — it just counts flags.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('tbm', 'checklist.min_tier', '"ok"',
   'P23d — minimum tier a TBM setup must reach before default GUI/alert surfaces pick it up. Ladder: elite(5) > strong(4) > ok(3) > weak(2) > filtered(≤1). Set to "weak" to see early forming setups, "strong" to only surface high-conviction reversals.'),
  ('tbm', 'checklist.min_score', '"3"',
   'P23d — numeric floor on the reversal checklist score (0–5). Used alongside checklist.min_tier; the stricter of the two wins. 3 = "at least 3 of 5 textbook legs present".')
ON CONFLICT (module, config_key) DO NOTHING;
