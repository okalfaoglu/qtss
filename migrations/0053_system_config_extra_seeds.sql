-- Additional `system_config` seeds (non-secret).
-- Keep secrets in env (DATABASE_URL, QTSS_JWT_SECRET, provider API keys).

INSERT INTO system_config (module, config_key, value, schema_version, description, is_secret)
VALUES
    -- API
    ('api', 'bind', '{"value":"0.0.0.0:8080"}'::jsonb, 1, 'HTTP bind address for qtss-api.', false),
    ('api', 'jwt_audience', '{"value":"qtss-api"}'::jsonb, 1, 'JWT aud claim.', false),
    ('api', 'jwt_issuer', '{"value":"qtss"}'::jsonb, 1, 'JWT iss claim.', false),
    ('api', 'jwt_access_ttl_secs', '{"value":"900"}'::jsonb, 1, 'JWT access token TTL seconds.', false),
    ('api', 'jwt_refresh_ttl_secs', '{"value":"2592000"}'::jsonb, 1, 'JWT refresh token TTL seconds.', false),
    ('api', 'rate_limit_replenish_ms', '{"value":"20"}'::jsonb, 1, 'tower-governor replenish (ms).', false),
    ('api', 'rate_limit_burst', '{"value":"120"}'::jsonb, 1, 'tower-governor burst size.', false),
    ('api', 'audit_http_enabled', '{"enabled": false}'::jsonb, 1, 'Enable audit_log for HTTP mutations (requires admin).', false),
    ('api', 'trusted_proxies_csv', '{"value":""}'::jsonb, 1, 'Comma-separated trusted proxy IP/CIDR list (ForwardedIpKeyExtractor).', false),
    ('api', 'metrics_token', '{"value":""}'::jsonb, 1, 'Optional token required for GET /metrics.', false),
    ('seed', 'admin_email', '{"value":"admin@localhost"}'::jsonb, 1, 'Default admin email for qtss-seed.', false),

    -- Worker: kline and probe HTTP
    ('worker', 'kline_interval', '{"value":"1m"}'::jsonb, 1, 'Kline interval for Binance WS (1m/15m/4h...).', false),
    ('worker', 'kline_segment', '{"value":"spot"}'::jsonb, 1, 'Kline segment for Binance WS (spot|futures).', false),
    ('worker', 'kline_symbol', '{"value":""}'::jsonb, 1, 'Single kline symbol (e.g., BTCUSDT) when kline_symbols is empty.', false),
    ('worker', 'kline_symbols_csv', '{"value":""}'::jsonb, 1, 'Comma-separated symbols for combined kline WS.', false),
    ('worker', 'http_bind', '{"value":""}'::jsonb, 1, 'Optional worker probe HTTP bind address (e.g., 127.0.0.1:9090).', false),

    -- Worker: notify feature flags and channels (credentials remain env for qtss-notify)
    ('worker', 'notify_outbox_enabled', '{"enabled": false}'::jsonb, 1, 'Enable notify_outbox consumer loop.', false),
    ('worker', 'notify_outbox_tick_secs', '{"secs": 10}'::jsonb, 1, 'notify_outbox consumer tick seconds.', false),
    ('worker', 'paper_position_notify_enabled', '{"enabled": false}'::jsonb, 1, 'Enable paper (dry) fill notifications.', false),
    ('worker', 'paper_position_notify_channels_csv', '{"value":"telegram"}'::jsonb, 1, 'Channels for paper fill notifications.', false),
    ('worker', 'paper_position_notify_tick_secs', '{"secs": 30}'::jsonb, 1, 'Paper fill notify loop tick seconds.', false),
    ('worker', 'live_position_notify_enabled', '{"enabled": false}'::jsonb, 1, 'Enable live exchange fill notifications.', false),
    ('worker', 'live_position_notify_channels_csv', '{"value":"telegram"}'::jsonb, 1, 'Channels for live fill notifications.', false),
    ('worker', 'live_position_notify_tick_secs', '{"secs": 45}'::jsonb, 1, 'Live fill notify loop tick seconds.', false),

    -- Worker: kill switch (numbers stored as strings; parsed in worker)
    ('worker', 'kill_switch_enabled', '{"enabled": false}'::jsonb, 1, 'Enable kill switch loop.', false),
    ('worker', 'kill_switch_db_sync_tick_secs', '{"secs": 5}'::jsonb, 1, 'Kill switch app_config sync poll tick.', false),
    ('worker', 'kill_switch_pnl_poll_tick_secs', '{"secs": 60}'::jsonb, 1, 'Kill switch PnL poll tick.', false),
    ('worker', 'kill_switch_reference_equity_usdt', '{"value":"100000"}'::jsonb, 1, 'Reference equity for drawdown percentage.', false),
    ('worker', 'max_drawdown_pct', '{"value":"5.0"}'::jsonb, 1, 'Max drawdown percent (e.g., 5.0).', false),
    ('worker', 'kill_switch_daily_loss_usdt', '{"value":"1000000"}'::jsonb, 1, 'Daily loss trigger (USDT) if drawdown pct is not set.', false)
ON CONFLICT (module, config_key) DO NOTHING;

