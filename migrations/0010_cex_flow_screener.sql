-- 0010_cex_flow_screener.sql
-- Derived CEX flow screeners from `data_snapshots` key `nansen_netflows`.
-- Toggle accumulation vs distribution jobs and optional Telegram via `notify_outbox`.

INSERT INTO system_config (module, config_key, value, description, is_secret)
VALUES
    ('worker', 'cex_flow_accumulation_screener_enabled', '{"enabled": false}'::jsonb,
     'Build `cex_flow_accumulation_top25` snapshot (CEX outflow / accumulation ranking)', false),
    ('worker', 'cex_flow_distribution_screener_enabled', '{"enabled": false}'::jsonb,
     'Build `cex_flow_distribution_top25` snapshot (CEX inflow / dump-risk ranking)', false),
    ('worker', 'cex_flow_screener_tick_secs', '{"secs": 3600}'::jsonb,
     'Poll interval for screener loop (writes `data_snapshots` when upstream netflows exist)', false),
    ('worker', 'cex_flow_screener_top_n', '{"value": 25}'::jsonb,
     'TOP N per report (clamped 5–100 in worker)', false),
    ('worker', 'cex_flow_accumulation_notify_enabled', '{"enabled": false}'::jsonb,
     'Enqueue Telegram/outbox for accumulation report (same tick dedup via event_key)', false),
    ('worker', 'cex_flow_distribution_notify_enabled', '{"enabled": false}'::jsonb,
     'Enqueue Telegram/outbox for distribution report', false),
    ('worker', 'cex_flow_screener_notify_channels_csv', '{"value": "telegram"}'::jsonb,
     'Channels CSV for screener notifications (e.g. telegram)', false)
ON CONFLICT (module, config_key) DO UPDATE SET
    description = EXCLUDED.description,
    is_secret = EXCLUDED.is_secret,
    updated_at = now();
