-- 0180_faz9_min_target_price_floor.sql
--
-- bug_negative_target_price.md — TP/SL hard price floor.
--
-- Background: RAVEUSDT 1h SHORT setup (entry 1.18133, SL 1.747431) had
-- TP1 rendered as -0.23392268 in the Telegram card. Root cause: the
-- adaptive R-multiple ladder (entry - k*R) on a wide-SL short produced
-- sub-zero target prices that no downstream guard caught. Fix
-- (qtss-wyckoff::trade_planner) clamps the P&F cap to a positive floor
-- and drops any rung below `entry * min_target_price_frac`. If TP1
-- itself drops, the setup is rejected with `NegativeTargetProjection`.
--
-- Default 0.001 (= 0.1% of entry) is conservative — high-precision
-- tokens (e.g. SHIB at 1e-5) still fall comfortably above it.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('setup', 'wyckoff.plan.min_target_price_frac', '0.001'::jsonb,
   'TP/SL hard floor as fraction of entry price. Guards against R-multiple short ladders projecting sub-zero targets (bug_negative_target_price.md). Setups whose TP1 drops below this floor are rejected with NegativeTargetProjection.')
ON CONFLICT (module, config_key) DO NOTHING;
