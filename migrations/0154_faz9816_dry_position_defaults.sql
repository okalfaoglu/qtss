-- Faz 9.8.16 — dry-mode position defaults.
--
-- The execution_bridge seeds paper fills into `live_positions`. Without
-- leverage + liquidation_price the tick dispatcher's liquidation guard
-- short-circuits (spot can't liquidate, and missing liq price aborts
-- the assessment). These knobs let the dry path mirror a realistic
-- futures position so guard telemetry + training data accumulate.

SELECT _qtss_register_key(
    'execution.dry.default_leverage', 'execution', 'dry',
    'int', '10'::jsonb, '',
    'Leverage assumed for dry-mode futures positions (10x Binance default).',
    'number', false, 'normal', ARRAY['execution','faz9816','dry']
);

SELECT _qtss_register_key(
    'execution.dry.maint_margin_ratio', 'execution', 'dry',
    'float', '0.005'::jsonb, '',
    'Maintenance margin ratio (Binance USDT-M majors ≈ 0.5%).',
    'number', false, 'normal', ARRAY['execution','faz9816','dry']
);

SELECT _qtss_register_key(
    'execution.dry.default_segment', 'execution', 'dry',
    'string', '"futures"'::jsonb, '',
    'Fallback market segment when selector_meta.venue_class is missing (spot|futures|margin|options).',
    'text', false, 'normal', ARRAY['execution','faz9816','dry']
);
