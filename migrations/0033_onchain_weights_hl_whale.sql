-- Optional weight for Nansen whale / profiler aggregate → `hl_whale_score` pillar.
UPDATE app_config
SET value = value || '{"hl_whale": 1.0}'::jsonb
WHERE key = 'onchain_signal_weights';

-- Optional map: engine symbol (uppercase) → { "token_address": "0x...", "chain": "ethereum" } for flow-intelligence loop.
INSERT INTO app_config (key, value, description)
VALUES (
    'nansen_flow_intel_by_symbol',
    '{}'::jsonb,
    'Per-symbol bodies for Nansen tgm/flow-intelligence (worker nansen_extended)'
)
ON CONFLICT (key) DO NOTHING;
