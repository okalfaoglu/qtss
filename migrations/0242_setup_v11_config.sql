-- Setup v1.1 config — live-tick entry + SL-hit cooldown.
--
-- The allocator now prefers a side-aware bookTicker price (long→best_ask,
-- short→best_bid) over the last 15-min bar close. That alone closes the
-- common whipsaw path, but a secondary guard is still needed: after an
-- SL hit the same (symbol, direction) is locked out for N minutes so
-- the pipeline cannot re-arm into the same losing spot immediately.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('allocator_v2', 'sl_hit_cooldown_minutes',
     '{"value": 15}'::jsonb,
     'After an SL hit on (symbol, direction), suppress new arms for this many minutes. 0 disables the guard; 15 kills same-direction re-arm whipsaw without blocking an honest regime flip (opposite direction is unaffected).')
ON CONFLICT (module, config_key) DO NOTHING;
