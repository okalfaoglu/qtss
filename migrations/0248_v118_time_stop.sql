-- Allocator v1.1.8 — time-stop (edge-decay) loop config seeds.
--
-- ChatGPT teardown item #7: a setup whose TP has not hit within a
-- sane number of bars is probably in chop — close it and free the
-- capital instead of grinding to SL.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('allocator_v2', 'time_stop.enabled',
     '{"enabled": true}'::jsonb,
     'Master on/off for the time-stop loop. When true, armed setups without a TP1 hit are closed once they exceed the per-profile max-bar threshold.'),
    ('allocator_v2', 'time_stop.max_bars_t',
     '{"value": 12}'::jsonb,
     'T-profile (short-horizon 5m/15m/30m) max age in bars before time_stop fires. 12 bars ≈ 3 hours on 15m.'),
    ('allocator_v2', 'time_stop.max_bars_d',
     '{"value": 24}'::jsonb,
     'D-profile (1h/4h/1d/1w) max age in bars before time_stop fires. 24 bars ≈ a day on 1h or four days on 4h.')
ON CONFLICT (module, config_key) DO NOTHING;
