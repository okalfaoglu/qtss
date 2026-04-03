-- Vite dev/preview: proxy target for /api, /oauth, /health (qtss-api base URL reachable from the Node process).
-- Read by web/vite.config.ts when DATABASE_URL is set. Env override: QTSS_CONFIG_ENV_OVERRIDES=1 + QTSS_API_PROXY_TARGET.
INSERT INTO system_config (module, config_key, value, description, is_secret)
VALUES (
        'api',
        'web_dev_proxy_target',
        '{"value":"http://127.0.0.1:8080"}'::jsonb,
        'Vite proxy upstream (qtss-api). Set to WSL IP:8080 if preview runs on Windows and API runs in WSL.',
        false
    )
ON CONFLICT (module, config_key) DO NOTHING;
