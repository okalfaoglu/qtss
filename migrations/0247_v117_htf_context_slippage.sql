-- Allocator v1.1.7 — HTF context gate + dry-fill slippage sim.
--
-- Both independent code reviews (ChatGPT + Gemini) called out the
-- paper-vs-live execution gap and the lack of HTF alignment in the
-- decision stack. This migration seeds the two new config rows:
--
-- 1. allocator_v2.htf_context_gate_enabled — when true, the allocator
--    refuses to arm a lower-TF setup that contradicts a strong_*
--    verdict on the corresponding higher timeframes.
-- 2. execution.dry.slippage_bps — basis-point nudge applied to paper
--    fills so the realized entry is a realistic taker + half-spread
--    adjustment away from the idealised bookTicker mid.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('allocator_v2', 'htf_context_gate_enabled',
     '{"enabled": true}'::jsonb,
     'When true, allocator rejects LTF setups whose HTF consensus (within the candidate TF''s htf_lookup_set) is strong in the opposite direction.'),
    ('execution', 'dry.slippage_bps',
     '{"value": 2.0}'::jsonb,
     'Basis points of simulated slippage + half-spread applied to paper fills (long pushes entry up, short pushes it down). 2 bps ≈ 0.02 percent, realistic for Binance USDT perp taker. Set to 0 to disable the nudge and keep ideal paper entries.')
ON CONFLICT (module, config_key) DO NOTHING;
