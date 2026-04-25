-- Allocator v1.2.2 — best-of-tick filter.
--
-- ChatGPT teardown: "aynı anda 5 setup varsa sadece en güçlü 1 tanesini
-- al". Hard 1-only is too restrictive when multiple symbols genuinely
-- diverge; instead cap at N (default 3) and process them in confidence
-- DESC order so the cap keeps the strongest signals.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('allocator_v2', 'max_armed_per_tick',
     '{"value": 3}'::jsonb,
     'Max number of setups armed per allocator tick across all candidates. Candidates are processed in confidence DESC order; the strongest N pass through, the rest are deferred. Set to 0 to disable.')
ON CONFLICT (module, config_key) DO NOTHING;
