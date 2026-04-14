-- 0067_wyckoff_tf_guards_and_variants.sql — Faz 10 / P1a.
--
-- Two new guards for the Wyckoff detector:
--
-- 1) Timeframe-appropriate range filters
--    Without these, an H1 detector can surface a multi-month D1-scale
--    range as "valid Wyckoff" — producing setups with absurd TP3
--    values (e.g. 15% on 1h is almost always a stale broader structure,
--    not an active range).
--
--      max_range_height_pct : reject ranges whose height / midpoint
--                             exceeds this fraction (e.g. 0.08 on H1)
--      max_range_age_bars   : reject ranges whose first-to-last pivot
--                             span exceeds this many bars
--
--    Per-TF overrides can be added later as
--    `detector.wyckoff.max_range_height_pct.1h` / `.4h` / `.1d` — the
--    resolver falls back to the base key if the TF-suffixed row is
--    absent.
--
-- 2) Spring variant classification (Pruden, "Three Skills of Top Trading")
--    Each Spring is #1 Terminal / #2 Ordinary / #3 No-Supply by the
--    volume of the Spring bar relative to the pivot-window average:
--      - Terminal  : volume >= spring_terminal_vol_ratio × avg  (weakest)
--      - Ordinary  : in between                                 (baseline)
--      - No-Supply : volume <= spring_no_supply_vol_ratio × avg (strongest)
--    The No-Supply Spring is the highest-probability Wyckoff entry
--    because it confirms sellers are exhausted. Terminal Springs are
--    aggressive and statistically weaker; we skip them by default.

INSERT INTO system_config (module, config_key, value, description) VALUES
  -- TF guards (base defaults; tune per-TF after backtesting)
  ('detector', 'wyckoff.max_range_height_pct', '"0.15"',
   'Reject ranges whose height / midpoint exceeds this fraction. Base default 0.15; set 0.08 on H1, 0.30 on D1.'),
  ('detector', 'wyckoff.max_range_age_bars', '"500"',
   'Reject ranges whose pivot span exceeds N bars. Stale range guard.'),
  -- Spring variant classification
  ('detector', 'wyckoff.spring_no_supply_vol_ratio', '"0.8"',
   'Spring bar volume <= N × avg_vol → #3 No-Supply (strongest variant, score +25%).'),
  ('detector', 'wyckoff.spring_terminal_vol_ratio', '"3.0"',
   'Spring bar volume >= N × avg_vol → #1 Terminal (weakest variant, score -30%).'),
  ('detector', 'wyckoff.skip_terminal_springs', '"true"',
   'Skip #1 Terminal Springs entirely. Default true per Pruden: they are aggressive and low edge.')
ON CONFLICT (module, config_key) DO NOTHING;
