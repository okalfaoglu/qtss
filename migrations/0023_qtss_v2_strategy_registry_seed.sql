-- 0023_qtss_v2_strategy_registry_seed.sql
--
-- Faz 7 Adım 11 — Strategy registry config seed.
--
-- The pattern→strategy bridge in qtss-worker reads its provider list +
-- per-provider tunables out of system_config so an operator can rewire
-- the registry without a deploy. CLAUDE.md #1: each provider has its
-- own enable flag (no central match arm); CLAUDE.md #2: every knob
-- lives in the table.

SELECT _qtss_register_key('strategy.confidence_threshold.enabled', 'strategy','confidence_threshold','bool',
    'true'::jsonb, NULL,
    'Master switch for the v2 ConfidenceThresholdStrategy provider in the pattern→strategy bridge.',
    'toggle', true, 'normal', ARRAY['strategy','confidence_threshold']);

SELECT _qtss_register_key('strategy.confidence_threshold.min_confidence', 'strategy','confidence_threshold','float',
    '0.6'::jsonb, NULL,
    'Validated detection confidence floor before this provider emits an intent (0..1).',
    'number', true, 'normal', ARRAY['strategy','confidence_threshold']);

SELECT _qtss_register_key('strategy.confidence_threshold.risk_pct', 'strategy','confidence_threshold','float',
    '0.005'::jsonb, NULL,
    'Equity fraction risked per intent emitted by this provider (0.005 = 0.5%).',
    'number', true, 'normal', ARRAY['strategy','confidence_threshold']);

SELECT _qtss_register_key('strategy.confidence_threshold.time_in_force', 'strategy','confidence_threshold','string',
    '"gtc"'::jsonb, NULL,
    'TimeInForce for emitted intents: gtc | ioc | fok | day.',
    'enum', false, 'normal', ARRAY['strategy','confidence_threshold']);

SELECT _qtss_register_key('strategy.confidence_threshold.time_stop_secs', 'strategy','confidence_threshold','int',
    '0'::jsonb, 'seconds',
    'Optional time-stop on emitted intents; 0 disables the time stop.',
    'number', false, 'normal', ARRAY['strategy','confidence_threshold']);

SELECT _qtss_register_key('strategy.confidence_threshold.act_on_forming', 'strategy','confidence_threshold','bool',
    'false'::jsonb, NULL,
    'When true the provider emits intents on forming detections, not just confirmed ones.',
    'toggle', false, 'normal', ARRAY['strategy','confidence_threshold']);

SELECT _qtss_register_key('worker.runtime_mode', 'worker','runtime','string',
    '"dry"'::jsonb, NULL,
    'Worker execution mode: live | dry | backtest. Drives StrategyContext.run_mode and orchestrator persistence.',
    'enum', true, 'normal', ARRAY['worker','runtime']);
