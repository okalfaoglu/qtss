-- CEX flow screener: ensure keys exist, then enable reports + notify + tick + top_n.
-- Run: psql "$DATABASE_URL" -f docs/sql/cex_flow_screener_enable.sql
-- Post-deploy checklist: docs/sql/cex_flow_screener_todo.txt

-- 1) Insert missing rows (idempotent; does not overwrite existing value).
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

-- 2) Birikim raporu (CEX outflow / accumulation)
UPDATE system_config
SET value = '{"enabled": true}'::jsonb, updated_at = now()
WHERE module = 'worker' AND config_key = 'cex_flow_accumulation_screener_enabled';

-- Dağıtım raporu (CEX inflow / dump risk)
UPDATE system_config
SET value = '{"enabled": true}'::jsonb, updated_at = now()
WHERE module = 'worker' AND config_key = 'cex_flow_distribution_screener_enabled';

-- Döngü periyodu (saniye)
UPDATE system_config
SET value = '{"secs": 3600}'::jsonb, updated_at = now()
WHERE module = 'worker' AND config_key = 'cex_flow_screener_tick_secs';

-- TOP N (5–100 arası mantıklı)
UPDATE system_config
SET value = '{"value": 25}'::jsonb, updated_at = now()
WHERE module = 'worker' AND config_key = 'cex_flow_screener_top_n';

-- Telegram kuyruğu (notify_outbox) — ayrı ayrı
UPDATE system_config
SET value = '{"enabled": true}'::jsonb, updated_at = now()
WHERE module = 'worker' AND config_key = 'cex_flow_accumulation_notify_enabled';

UPDATE system_config
SET value = '{"enabled": true}'::jsonb, updated_at = now()
WHERE module = 'worker' AND config_key = 'cex_flow_distribution_notify_enabled';

-- Bildirim kanalları
UPDATE system_config
SET value = '{"value": "telegram"}'::jsonb, updated_at = now()
WHERE module = 'worker' AND config_key = 'cex_flow_screener_notify_channels_csv';
