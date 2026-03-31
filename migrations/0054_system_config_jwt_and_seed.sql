-- Add system_config keys for JWT and seed bootstrap (non-secret values).
-- NOTE: Do not store DATABASE_URL in DB (bootstrap). Secrets are persisted as `is_secret=true`.

INSERT INTO system_config (module, config_key, value, schema_version, description, is_secret)
VALUES
    ('api', 'jwt_audience', '{"value":"qtss-api"}'::jsonb, 1, 'JWT aud claim.', false),
    ('api', 'jwt_issuer', '{"value":"qtss"}'::jsonb, 1, 'JWT iss claim.', false),
    ('api', 'jwt_access_ttl_secs', '{"value":"900"}'::jsonb, 1, 'JWT access token TTL seconds.', false),
    ('api', 'jwt_refresh_ttl_secs', '{"value":"2592000"}'::jsonb, 1, 'JWT refresh token TTL seconds.', false),
    ('seed', 'admin_email', '{"value":"admin@localhost"}'::jsonb, 1, 'Default admin email for qtss-seed.', false)
ON CONFLICT (module, config_key) DO NOTHING;

