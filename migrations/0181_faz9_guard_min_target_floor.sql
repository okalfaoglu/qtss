-- 0181_faz9_guard_min_target_floor.sql
--
-- bug_negative_target_price.md follow-up — extend the price floor
-- guard from qtss-wyckoff::trade_planner (migration 0180) to the
-- ATR-fallback PositionGuard in qtss-setup-engine. Same vector:
-- short setup with a wide ATR stop produces target_ref =
-- entry - stop_distance * target_ref_r, which can dive below zero
-- on cheap tokens. Per-profile (T/Q/D) so operators can tighten the
-- floor on micro-cap perps without touching scalp profiles.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('setup', 'profile.t.min_target_price_frac', '0.001'::jsonb,
   'T (Trader) profile — TP/SL floor as fraction of entry. Guards PositionGuard ATR fallback against negative target_ref. See bug_negative_target_price.md.'),
  ('setup', 'profile.q.min_target_price_frac', '0.001'::jsonb,
   'Q (Quant) profile — TP/SL floor as fraction of entry.'),
  ('setup', 'profile.d.min_target_price_frac', '0.001'::jsonb,
   'D (Discretionary) profile — TP/SL floor as fraction of entry.')
ON CONFLICT (module, config_key) DO NOTHING;
