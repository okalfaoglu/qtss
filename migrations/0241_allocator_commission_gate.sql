-- Allocator commission viability gate config.
--
-- The allocator now rejects setups whose first take-profit cannot cover
-- `safety_multiple × round_trip_taker_pct`. This prevents the pipeline
-- from arming negative-expectancy trades (TP smaller than round-trip
-- commission) that historically polluted the RADAR winrate.
--
-- The multiplier lives in its own row so operators can tune the
-- "commission cushion" without touching the underlying fee schedule.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('allocator_v2', 'commission.safety_multiple',
     '{"value": 2.0}'::jsonb,
     'Minimum ratio of first take-profit move (|tp1-entry|/entry) to round-trip taker commission. 2.0 means TP must be at least twice the round-trip fees before the setup can arm.')
ON CONFLICT (module, config_key) DO NOTHING;
