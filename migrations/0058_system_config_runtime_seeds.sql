-- FAZ 11: Extra `system_config` / `app_config` seeds (idempotent).
-- Schema: `0044_system_config.sql`. Env fallback: `QTSS_CONFIG_ENV_OVERRIDES=1`.
-- Not: Duplicated `0013_*.sql` önekleri SQLx sürüm çakışması yaratır; worker çekirdeği `0013_worker_analytics_schema.sql` kalır.

-- api
INSERT INTO system_config (module, config_key, value, description, is_secret) VALUES
('api', 'jwt_audience', '{"value":"qtss-api"}', 'JWT aud', false),
('api', 'jwt_issuer', '{"value":"qtss"}', 'JWT iss', false),
('api', 'jwt_access_ttl_secs', '{"value":"900"}', 'Access TTL seconds', false),
('api', 'jwt_refresh_ttl_secs', '{"value":"2592000"}', 'Refresh TTL seconds', false),
('api', 'bind', '{"value":"0.0.0.0:8080"}', 'HTTP bind', false),
('api', 'rate_limit_replenish_ms', '{"value":"20"}', 'Governor replenish ms', false),
('api', 'rate_limit_burst', '{"value":"120"}', 'Governor burst', false),
('api', 'metrics_token', '{"value":""}', 'Optional /metrics token', false),
('api', 'trusted_proxies_csv', '{"value":""}', 'Trusted reverse proxies CIDR list', false),
('api', 'audit_http_enabled', '{"enabled":false}', 'HTTP mutation audit_log', false)
ON CONFLICT (module, config_key) DO NOTHING;

-- seed (qtss-seed — admin_password / oauth_client_secret satırları seed tarafından oluşturulur)
INSERT INTO system_config (module, config_key, value, description, is_secret) VALUES
('seed', 'admin_email', '{"value":"admin@localhost"}', 'Admin email', false),
('seed', 'binance_spot_api_key', '{"value":""}', 'Optional seed → exchange_accounts', true),
('seed', 'binance_spot_api_secret', '{"value":""}', 'Optional seed → exchange_accounts', true),
('seed', 'binance_futures_api_key', '{"value":""}', 'Optional seed → exchange_accounts', true),
('seed', 'binance_futures_api_secret', '{"value":""}', 'Optional seed → exchange_accounts', true)
ON CONFLICT (module, config_key) DO NOTHING;

-- worker — kline / rollup / Nansen
INSERT INTO system_config (module, config_key, value, description, is_secret) VALUES
('worker', 'pnl_rollup_tick_secs', '{"secs":300}', 'PnL rollup loop', false),
('worker', 'market_data_exchange', '{"value":"binance"}', 'Kline venue', false),
('worker', 'kline_interval', '{"value":"1m"}', 'Default kline interval', false),
('worker', 'kline_segment', '{"value":"spot"}', 'Default kline segment', false),
('worker', 'kline_symbols_csv', '{"value":""}', 'Comma symbols override', false),
('worker', 'kline_symbol', '{"value":""}', 'Single symbol fallback', false),
('worker', 'http_bind', '{"value":""}', 'Worker probe HTTP bind', false),
('worker', 'nansen_enabled', '{"enabled":true}', 'Nansen master switch', false),
('worker', 'nansen_token_screener_tick_secs', '{"secs":1800}', 'Token screener poll', false),
('worker', 'nansen_insufficient_credits_sleep_secs', '{"secs":3600}', '402/403 backoff', false),
('worker', 'nansen_api_base', '{"value":"https://api.nansen.ai"}', 'Nansen REST base', false),
('worker', 'nansen_api_key', '{"value":""}', 'Nansen API key', true),
('worker', 'notify_outbox_enabled', '{"enabled":false}', 'Drain notify_outbox', false),
('worker', 'notify_outbox_tick_secs', '{"secs":10}', 'Outbox poll', false),
('worker', 'kill_switch_db_sync_tick_secs', '{"secs":5}', 'app_config halt sync', false),
('worker', 'kill_switch_enabled', '{"enabled":false}', 'P&L-based kill loop', false),
('worker', 'kill_switch_pnl_poll_tick_secs', '{"secs":60}', 'Kill loop poll', false),
('worker', 'max_drawdown_pct', '{"value":"5.0"}', 'Drawdown % vs reference equity', false),
('worker', 'kill_switch_reference_equity_usdt', '{"value":"100000"}', 'Reference equity USDT', false),
('worker', 'kill_switch_daily_loss_usdt', '{"value":"1000000"}', 'Daily loss cap (if no drawdown %)', false),
('worker', 'position_manager_enabled', '{"enabled":false}', 'Position manager', false),
('worker', 'position_manager_tick_secs', '{"secs":10}', 'Position manager poll', false),
('worker', 'position_manager_bar_interval', '{"value":"1m"}', 'Mark price from bars', false),
('worker', 'position_manager_dry_close_enabled', '{"enabled":false}', 'Dry reduce-only exit', false),
('worker', 'position_manager_live_close_enabled', '{"enabled":false}', 'Live reduce-only exit', false),
('worker', 'position_manager_trailing_on_directive', '{"enabled":false}', 'Trailing only on AI directive', false),
('worker', 'position_manager_managed_trailing_enabled', '{"enabled":false}', 'Managed trailing stops', false),
('worker', 'position_manager_managed_trailing_callback_rate_pct', '{"value":"1"}', 'Trailing callback %', false),
('worker', 'position_manager_managed_trailing_limit_offset_pct', '{"value":"0.2"}', 'Limit offset %', false),
('worker', 'position_manager_managed_trailing_replace_step_pct', '{"value":"0.1"}', 'Replace step %', false),
('worker', 'default_stop_loss_pct', '{"value":"2.0"}', 'Default SL %', false),
('worker', 'default_take_profit_pct', '{"value":"4.0"}', 'Default TP %', false),
('worker', 'default_leverage', '{"value":"3"}', 'Default leverage hint', false),
('worker', 'reconcile_binance_spot_enabled', '{"enabled":false}', 'Spot open-order reconcile', false),
('worker', 'reconcile_binance_spot_tick_secs', '{"secs":3600}', 'Spot reconcile poll', false),
('worker', 'reconcile_binance_spot_patch_status', '{"enabled":true}', 'Patch submitted→reconciled_not_open', false),
('worker', 'reconcile_binance_spot_refine_order_status', '{"enabled":false}', 'GET /order refine', false),
('worker', 'reconcile_binance_spot_refine_max', '{"value":"30"}', 'Max refine queries', false),
('worker', 'reconcile_binance_futures_enabled', '{"enabled":false}', 'Futures reconcile', false),
('worker', 'reconcile_binance_futures_tick_secs', '{"secs":3600}', 'Futures reconcile poll', false),
('worker', 'reconcile_binance_futures_patch_status', '{"enabled":true}', 'Futures patch status', false),
('worker', 'reconcile_binance_futures_refine_order_status', '{"enabled":false}', 'Futures refine', false),
('worker', 'reconcile_binance_futures_refine_max', '{"value":"30"}', 'Futures refine max', false),
('worker', 'ai_expire_stale_decisions_tick_secs', '{"secs":300}', 'Expire stale AI rows', false),
('worker', 'ai_engine_worker_enabled', '{"enabled":true}', 'Spawn qtss-ai background tasks in worker', false),
('worker', 'strategy_runner_enabled', '{"enabled":false}', 'Dry strategy runner', false),
('worker', 'paper_ledger_enabled', '{"enabled":false}', 'Paper ledger persist', false),
('worker', 'paper_org_id', '{"value":""}', 'Paper org UUID', false),
('worker', 'paper_user_id', '{"value":""}', 'Paper user UUID', false),
('worker', 'strategy_runner_quote_balance_usdt', '{"value":"100000"}', 'Total dry quote balance', false),
('worker', 'strategy_signal_filter_balance', '{"value":""}', 'Optional per-strategy balance', false),
('worker', 'strategy_whale_momentum_balance', '{"value":""}', 'Optional per-strategy balance', false),
('worker', 'strategy_arb_funding_balance', '{"value":""}', 'Optional per-strategy balance', false),
('worker', 'strategy_copy_trade_balance', '{"value":""}', 'Optional per-strategy balance', false)
ON CONFLICT (module, config_key) DO NOTHING;

-- strategy (qtss-strategy dry runner)
INSERT INTO system_config (module, config_key, value, description, is_secret) VALUES
('strategy', 'signal_filter_tick_secs', '{"secs":60}', 'signal_filter poll', false),
('strategy', 'signal_filter_auto_place', '{"enabled":false}', NULL, false),
('strategy', 'signal_filter_bracket_orders', '{"enabled":false}', NULL, false),
('strategy', 'strategy_order_qty', '{"value":"0.001"}', NULL, false),
('strategy', 'strategy_skip_human_approval', '{"enabled":false}', NULL, false),
('strategy', 'long_threshold', '{"value":"0.6"}', NULL, false),
('strategy', 'short_threshold', '{"value":"-0.6"}', NULL, false),
('strategy', 'min_signal_confidence', '{"value":"0.4"}', NULL, false),
('strategy', 'signal_filter_on_conflict', '{"value":"skip"}', 'skip|half', false),
('strategy', 'max_position_notional_usdt', '{"value":"10000"}', NULL, false),
('strategy', 'kelly_apply', '{"enabled":false}', NULL, false),
('strategy', 'kelly_win_rate', '{"value":"0.55"}', NULL, false),
('strategy', 'kelly_avg_win_loss_ratio', '{"value":"1.5"}', NULL, false),
('strategy', 'kelly_max_fraction', '{"value":"0.25"}', NULL, false),
('strategy', 'max_drawdown_pct', '{"value":"5.0"}', 'Drawdown guard', false),
('strategy', 'whale_momentum_tick_secs', '{"secs":120}', NULL, false),
('strategy', 'whale_momentum_threshold', '{"value":"0.45"}', NULL, false),
('strategy', 'whale_funding_crowding_block', '{"value":"0.0002"}', NULL, false),
('strategy', 'whale_momentum_auto_place', '{"enabled":false}', NULL, false),
('strategy', 'copy_trade_strategy_tick_secs', '{"secs":120}', NULL, false),
('strategy', 'copy_trade_direction_threshold', '{"value":"0.25"}', NULL, false),
('strategy', 'copy_trade_base_qty', '{"value":"0.001"}', NULL, false),
('strategy', 'copy_trade_default_symbol', '{"value":"BTCUSDT"}', NULL, false),
('strategy', 'copy_trade_bar_exchange', '{"value":"binance"}', NULL, false),
('strategy', 'copy_trade_bar_segment', '{"value":"futures"}', NULL, false),
('strategy', 'copy_trade_bar_interval', '{"value":"1m"}', NULL, false),
('strategy', 'copy_trade_strategy_auto_place', '{"enabled":false}', NULL, false),
('strategy', 'arb_funding_tick_secs', '{"secs":300}', NULL, false),
('strategy', 'arb_funding_threshold', '{"value":"0.0001"}', NULL, false),
('strategy', 'arb_funding_dry_two_leg', '{"enabled":false}', NULL, false),
('strategy', 'arb_funding_order_qty', '{"value":"0.001"}', NULL, false),
('strategy', 'arb_funding_symbol_base', '{"value":"btc"}', NULL, false),
('strategy', 'default_stop_loss_pct', '{"value":"2.0"}', 'Shared SL % for signal_filter', false),
('strategy', 'default_take_profit_pct', '{"value":"4.0"}', 'Shared TP % for signal_filter', false)
ON CONFLICT (module, config_key) DO NOTHING;

-- notify — qtss-notify JSON (`NotifyConfig`); kanalları admin API ile güncelleyin
INSERT INTO system_config (module, config_key, value, description, is_secret) VALUES
('notify', 'dispatcher_config', '{"telegram":null,"email":null,"sms":null,"whatsapp":null,"x":null,"facebook":null,"instagram":null,"discord":null,"webhook":null}', 'NotificationDispatcher channels', false)
ON CONFLICT (module, config_key) DO NOTHING;

-- ai — provider uçları (anahtarlar gizli)
INSERT INTO system_config (module, config_key, value, description, is_secret) VALUES
('ai', 'anthropic_api_key', '{"value":""}', 'ANTHROPIC_API_KEY', true),
('ai', 'anthropic_base_url', '{"value":"https://api.anthropic.com"}', NULL, false),
('ai', 'anthropic_timeout_secs', '{"secs":120}', NULL, false),
('ai', 'ollama_base_url', '{"value":"http://127.0.0.1:11434"}', NULL, false),
('ai', 'openai_compat_base_url', '{"value":""}', 'OpenAI-compatible /v1 base', false),
('ai', 'openai_compat_headers_json', '{"value":""}', 'Extra JSON headers', false),
('ai', 'onprem_timeout_secs', '{"secs":180}', NULL, false),
('ai', 'onprem_max_in_flight', '{"value":"4"}', NULL, false),
('ai', 'onprem_api_key', '{"value":""}', 'Optional Bearer for gateway', true)
ON CONFLICT (module, config_key) DO NOTHING;

-- app_config: ai_engine_config (tek satır JSON)
INSERT INTO app_config (key, value, description)
VALUES (
    'ai_engine_config',
    '{"enabled":false,"tactical_layer_enabled":true,"operational_layer_enabled":true,"strategic_layer_enabled":false,"auto_approve_threshold":0.85,"auto_approve_enabled":false,"tactical_tick_secs":900,"operational_tick_secs":120,"strategic_tick_secs":86400,"provider_tactical":"anthropic","provider_operational":"anthropic","provider_strategic":"anthropic","model_tactical":"claude-haiku-4-5-20251001","model_operational":"claude-haiku-4-5-20251001","model_strategic":"claude-sonnet-4-20250514","max_tokens_tactical":1024,"max_tokens_operational":512,"max_tokens_strategic":4096,"decision_ttl_secs":1800,"require_min_confidence":0.60,"output_locale":null}'::jsonb,
    'AI engine defaults (merged with system_config ai.* secrets at runtime)'
)
ON CONFLICT (key) DO NOTHING;
