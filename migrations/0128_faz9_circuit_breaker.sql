-- 0128_faz9_circuit_breaker.sql
--
-- Faz 9.4.3 — Circuit Breaker config keys.
--
-- Auto-disables the AI gate when model quality degrades:
-- excessive block rate or high PSI drift.

SELECT _qtss_register_key(
    'ai.circuit_breaker.enabled',
    'ai',
    'ai',
    'bool',
    'false'::jsonb,
    '',
    'Master switch for automatic circuit breaker (opt-in for safety)',
    'toggle',
    false,
    'normal',
    ARRAY['ai','circuit_breaker']
);

SELECT _qtss_register_key(
    'ai.circuit_breaker.min_predictions',
    'ai',
    'ai',
    'int',
    '50'::jsonb,
    '',
    'Minimum recent predictions before breaker can trip',
    'count',
    false,
    'normal',
    ARRAY['ai','circuit_breaker']
);

SELECT _qtss_register_key(
    'ai.circuit_breaker.max_block_rate',
    'ai',
    'ai',
    'float',
    '0.7'::jsonb,
    '',
    'If block rate exceeds this among recent predictions, trip the breaker',
    'ratio',
    false,
    'normal',
    ARRAY['ai','circuit_breaker']
);

SELECT _qtss_register_key(
    'ai.circuit_breaker.psi_trip_threshold',
    'ai',
    'ai',
    'float',
    '0.25'::jsonb,
    '',
    'If any feature PSI exceeds this, trip the breaker',
    'threshold',
    false,
    'normal',
    ARRAY['ai','circuit_breaker','psi']
);

SELECT _qtss_register_key(
    'ai.circuit_breaker.cooldown_minutes',
    'ai',
    'ai',
    'int',
    '60'::jsonb,
    '',
    'After tripping, wait this many minutes before re-evaluating',
    'minutes',
    false,
    'normal',
    ARRAY['ai','circuit_breaker']
);

SELECT _qtss_register_key(
    'ai.circuit_breaker.state',
    'ai',
    'ai',
    'string',
    '"closed"'::jsonb,
    '',
    'Current breaker state: closed (normal), open (tripped), half_open (testing)',
    'state',
    false,
    'normal',
    ARRAY['ai','circuit_breaker']
);

-- CHECK constraint on state values via config_schema validation.
-- The _qtss_register_key does not support CHECK directly; we enforce
-- at the application layer (closed|open|half_open).
