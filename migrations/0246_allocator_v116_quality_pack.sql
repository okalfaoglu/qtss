-- Allocator v1.1.6 — quality-over-quantity pack.
--
-- Config seeds for the six new gates introduced after independent
-- code reviews (ChatGPT + Gemini) flagged the same structural risks:
--   1. correlated detections inflating net_score (family clustering)
--   2. no daily trade ceiling (over-trading + fee farming)
--   3. loss-streak blindness (3 SL in a row, system keeps firing)
--   4. correlation-cluster duplication (BTC long + ETH long + SOL long
--      = one macro idea, not three independent edges)
--   5. no expected-value check (pattern detected != positive EV)
--
-- Family clustering is a code change in the confluence scorer; this
-- migration only carries the allocator-level knobs.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('allocator_v2', 'max_daily_armed',
     '{"value": 10}'::jsonb,
     'Hard cap on setups armed in the rolling last 24h across all symbols. Set to 0 to disable.'),
    ('allocator_v2', 'loss_streak_threshold',
     '{"value": 3}'::jsonb,
     'N consecutive sl_hit closes on the same (symbol, direction) trigger the extended ban.'),
    ('allocator_v2', 'loss_streak_ban_minutes',
     '{"value": 60}'::jsonb,
     'Ban duration when loss_streak_threshold is reached. 0 disables the gate.'),
    ('allocator_v2', 'corr_cluster_enabled',
     '{"enabled": true}'::jsonb,
     'When true, the correlation-cluster cap runs on every candidate.'),
    ('allocator_v2', 'corr_cluster_max_armed',
     '{"value": 2}'::jsonb,
     'Max armed setups in the same (cluster, direction) before a new candidate is skipped. Clusters: majors/l1s/memes/defi/payments.'),
    ('allocator_v2', 'ev_gate_enabled',
     '{"enabled": true}'::jsonb,
     'When true, allocator rejects candidates whose historical (symbol, direction, profile) expected value in percent is < ev_min_value_r.'),
    ('allocator_v2', 'ev_min_sample',
     '{"value": 10}'::jsonb,
     'Minimum closed-trade sample before the EV gate engages. Below this, EV gate is skipped (cold-start protection).'),
    ('allocator_v2', 'ev_min_value_r',
     '{"value": 0.0}'::jsonb,
     'EV floor in realized percent. 0 = break-even. Positive values demand an edge above zero.')
ON CONFLICT (module, config_key) DO NOTHING;
