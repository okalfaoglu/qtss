-- Faz 9.8.18 — per-mode account equity defaults.
--
-- The execution_bridge now sizes positions from `risk_pct` using the
-- canonical risk-per-trade formula:
--
--     qty = (equity * risk_pct) / |entry - sl|
--
-- Equity for dry is notional (paper book); equity for live should be
-- mirrored from the broker's wallet snapshot once Faz 9.8.19 wires a
-- balance fetcher. For now both are config-driven so the math is
-- auditable and operators can tune the paper book without code changes.

SELECT _qtss_register_key(
    'execution.dry.default_equity', 'execution', 'dry',
    'float', '10000'::jsonb, '',
    'Notional account equity (USDT) used to size dry-mode positions from risk_pct.',
    'number', false, 'normal', ARRAY['execution','faz9818','dry']
);

SELECT _qtss_register_key(
    'execution.live.default_equity', 'execution', 'live',
    'float', '1000'::jsonb, '',
    'Fallback account equity (USDT) for live sizing until the broker balance fetcher lands.',
    'number', false, 'normal', ARRAY['execution','faz9818','live']
);
