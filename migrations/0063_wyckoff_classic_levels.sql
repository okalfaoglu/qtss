-- Wyckoff classical levels — preserve pattern-native entry/SL/TP alongside adaptive Q-RADAR ladder.
-- L1 = Wyckoff classical (geometry-based, immutable per setup type)
-- L2 = Adaptive Q-RADAR (ATR + profile + score, volatility-scaled)
--
-- Final stored values: entry_price = L1, entry_sl = tighter of (L1, L2), tp_ladder = L2 capped by L1 P&F target.
-- We persist L1 separately so audits/research can compare classical vs adaptive performance.

BEGIN;

ALTER TABLE qtss_v2_setups
  ADD COLUMN IF NOT EXISTS wyckoff_classic JSONB;
-- Schema:
-- {
--   "setup_type":   "spring|lps|buec|ut|utad|lpsy|ice_retest",
--   "phase":        "C|D",
--   "range_top":    0.0,
--   "range_bottom": 0.0,
--   "range_height": 0.0,
--   "pnf_target":   0.0,           // P&F count projection (range_height * cause_effect_factor)
--   "entry":        0.0,           // classical trigger price
--   "sl":           0.0,           // classical invalidation price
--   "tp_targets":   [0.0, 0.0],    // classical Wyckoff TPs (range edges + P&F projection)
--   "trigger_event_id":  "uuid",   // wyckoff_event that triggered this setup (Spring/UT/SOS/SOW)
--   "trigger_bar_iso":   "...",
--   "volume_ratio":      0.0,
--   "spread_atr":        0.0,
--   "close_pos_pct":     0.0,
--   "wick_pct":          0.0
-- }

-- Cause/Effect factor for P&F target (Wyckoff Cause/Effect principle)
INSERT INTO system_config (module, config_key, value, description) VALUES
  ('setup', 'wyckoff.classic.cause_effect_factor', '"1.0"',
    'Multiplier on range_height for classical P&F target (Wyckoff Cause/Effect). 1.0 = full range projection.'),
  ('setup', 'wyckoff.classic.spring_buffer_atr', '"0.5"',
    'SL buffer below Spring low in ATR units (classical Wyckoff)'),
  ('setup', 'wyckoff.classic.lps_buffer_atr', '"0.3"',
    'SL buffer below LPS low in ATR units'),
  ('setup', 'wyckoff.classic.ut_buffer_atr', '"0.5"',
    'SL buffer above UT/UTAD high in ATR units'),
  ('setup', 'wyckoff.classic.lpsy_buffer_atr', '"0.3"',
    'SL buffer above LPSY high in ATR units'),
  ('setup', 'wyckoff.classic.buec_buffer_atr', '"0.4"',
    'SL buffer below creek for BUEC retest'),
  ('setup', 'wyckoff.classic.ice_retest_buffer_atr', '"0.4"',
    'SL buffer above ice for ice retest'),

  -- Sl selection policy: tighter|looser|classical_only|adaptive_only
  ('setup', 'wyckoff.sl.policy', '"tighter"',
    'How to combine classical and adaptive SL. tighter = min distance from entry (lower risk).'),

  -- TP cap policy: enforce range_height * cause_effect_factor as upper bound on adaptive ladder
  ('setup', 'wyckoff.tp.classical_cap_enabled', '"true"',
    'When true, no adaptive TP can exceed classical P&F target * range_cap_factor')
ON CONFLICT (module, config_key) DO NOTHING;

COMMIT;
