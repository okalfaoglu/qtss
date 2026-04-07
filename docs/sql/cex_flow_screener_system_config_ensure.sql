-- Manual / operator: CEX flow screener `system_config` rows (worker).
-- Use when migration 0010 has not run or UPDATE ... matched 0 rows.
-- Idempotent: missing keys only; existing `value` is never overwritten.
-- To insert + enable in one shot: `cex_flow_screener_enable.sql`.
-- Checklist: `cex_flow_screener_todo.txt`.

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
     'Enqueue Telegram/outbox for accumulation report (dedup via event_key)', false),
    ('worker', 'cex_flow_distribution_notify_enabled', '{"enabled": false}'::jsonb,
     'Enqueue Telegram/outbox for distribution report', false),
    ('worker', 'cex_flow_screener_notify_channels_csv', '{"value": "telegram"}'::jsonb,
     'Channels CSV for screener notifications (e.g. telegram)', false)
ON CONFLICT (module, config_key) DO NOTHING;

-- After rows exist, enable examples (uncomment as needed):
-- UPDATE system_config SET value = '{"enabled": true}'::jsonb, updated_at = now()
-- WHERE module = 'worker' AND config_key = 'cex_flow_accumulation_screener_enabled';
-- UPDATE system_config SET value = '{"enabled": true}'::jsonb, updated_at = now()
-- WHERE module = 'worker' AND config_key = 'cex_flow_distribution_screener_enabled';
