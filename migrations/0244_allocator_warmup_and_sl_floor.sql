-- Setup v1.1.2 — warm-up gate + minimum SL distance.
--
-- Root-cause analysis of the weekly RADAR report (8/9 losing trades,
-- all from `entry_source=bar_close_no_stream`) identified two gaps:
--
-- 1. On cold start the bookTicker WS is not yet connected when the
--    allocator fires its first tick, so every setup arms on bar_close
--    fallback. The next tick sees the real spot price and immediately
--    takes those arms to SL. `warmup_min_subscribers` blocks the
--    allocator loop until the tick store has at least this many live
--    symbols — eliminates the cold-start fiasco end-to-end.
--
-- 2. Tight SL distances (0.32-0.64% on 15m/1h) get stopped out by
--    plain intra-bar noise. `sl_min_distance_pct` rejects setups whose
--    SL sits inside typical variance range. 0.4% ≈ 4× round-trip
--    commission is the default — anything tighter is near-coin-flip.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('allocator_v2', 'warmup_min_subscribers',
     '{"value": 3}'::jsonb,
     'Minimum number of live symbols in the bookTicker PriceTickStore before the allocator will run a tick. Set to 0 to disable the warm-up gate (legacy behaviour).'),
    ('allocator_v2', 'sl_min_distance_pct',
     '{"value": 0.4}'::jsonb,
     'Minimum |entry - SL| / entry * 100 (percent). Setups with a tighter stop are rejected with reason=sl_too_tight. Typical intra-bar noise on 15m is 0.3-0.6%, so 0.4 is a conservative floor.')
ON CONFLICT (module, config_key) DO NOTHING;
