-- Nansen settings moved to system_config (secrets masked).

INSERT INTO system_config (module, config_key, value, schema_version, description, is_secret)
VALUES
    ('worker', 'nansen_api_base', '{"value":"https://api.nansen.ai"}'::jsonb, 1, 'Nansen API base URL.', false),
    ('worker', 'nansen_api_key', '{"value":""}'::jsonb, 1, 'Nansen API key (secret).', true),
    ('worker', 'nansen_insufficient_credits_sleep_secs', '{"secs":3600}'::jsonb, 1, 'Sleep seconds after insufficient credits (403).', false)
ON CONFLICT (module, config_key) DO NOTHING;

