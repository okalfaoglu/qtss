-- P24 — Effort vs Result (Wyckoff volume law) detector seed keys.
-- Scans last `scan_bars` bars ending at the TBM anchor for:
--   * no-supply down-bar (bottom hypothesis): sellers exhausted
--   * no-demand up-bar (top hypothesis): buyers exhausted
--   * absorption bar: high volume + small range + close mid-range
-- Bonus folds into the volume pillar score (capped by max_bonus_pts)
-- and the weighted TBM total is re-balanced.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('tbm', 'effort_result.enabled', '"1"',
   'P24 — master switch for the Wyckoff effort-vs-result detector. 1=on (adds volume-pillar bonus on no-supply/no-demand/absorption bars), 0=off.'),
  ('tbm', 'effort_result.scan_bars', '"8"',
   'P24 — trailing bars (ending at the anchor) scanned for effort-vs-result tells. 8 is a sensible window; raise to pick up older exhaustion bars, lower for pure anchor-vicinity scoring.'),
  ('tbm', 'effort_result.range_small_ratio', '"0.7"',
   'P24 — a bar''s total range must be ≤ this × 20-bar avg range to count as "small" (narrow-range bar, classic Wyckoff absorption/no-demand tell).'),
  ('tbm', 'effort_result.vol_low_ratio', '"0.8"',
   'P24 — volume gate for no-supply/no-demand: bar volume ≤ this × 20-bar avg. Lower = stricter (only truly dry-volume bars count).'),
  ('tbm', 'effort_result.vol_high_ratio', '"1.5"',
   'P24 — volume gate for absorption: bar volume ≥ this × 20-bar avg. Higher = stricter (only real effort bars count).'),
  ('tbm', 'effort_result.no_supply_demand_pts', '"10"',
   'P24 — points awarded per no-supply / no-demand bar found in the scan window.'),
  ('tbm', 'effort_result.absorption_pts', '"15"',
   'P24 — points awarded per absorption bar found in the scan window (effort-without-result is the strongest single tell).'),
  ('tbm', 'effort_result.max_bonus_pts', '"25"',
   'P24 — total cap on the effort-vs-result bonus (keeps stacked signals from blowing the volume pillar out).')
ON CONFLICT (module, config_key) DO NOTHING;
