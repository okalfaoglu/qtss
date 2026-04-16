-- 0122_faz9_commission_gate_multiple.sql
--
-- Faz 9.3 — commission buffer + net-of-commission outcome P&L.
--
-- Before: the open-time gate only rejected when expected profit <= round-trip
-- taker fee. `profit = 1.01 * round_trip` passed, and qtss_v2_setups.pnl_pct
-- was persisted gross — so the LightGBM trainer learned from a positively
-- biased label set (marginal trades whose fees ate the edge still labeled win).
--
-- After (qtss-worker/src/v2_setup_loop.rs):
--   * open gate:  profit_pct must exceed round_trip * min_profit_multiple
--   * close path: round_trip subtracted from pnl_pct before persistence,
--                 so outcome_labeler + Faz 9.3 features see net P&L.
-- Both paths share `resolve_commission_bps`, so venue overrides
-- (`setup.commission.{venue}.taker_bps`) still dominate the global fallback.

SELECT _qtss_register_key(
    'commission_gate.min_profit_multiple',
    'setup',
    'commission_gate',
    'float',
    '1.5'::jsonb,
    '',
    'Minimum ratio of expected profit %% to round-trip taker commission %% before a setup is allowed to open. 1.0 = pure break-even; 1.5 default leaves headroom for slippage + funding.',
    'ratio',
    false,
    'normal',
    ARRAY['setup','commission_gate','faz9']
);
