-- 0111_commission_sync.sql
--
-- Faz 8 step 3 — register config keys for the commission auto-refresh
-- loop (commission_sync_loop). The worker polls Binance signed endpoints
-- (`/fapi/v1/commissionRate`, `/sapi/v1/asset/tradeFee`) and upserts the
-- `commission.{venue_class}.{side}_bps` rows seeded in 0110, so VIP /
-- BNB-discount tiers propagate without a deploy (CLAUDE.md #2).
--
-- Disabled by default — needs a real `exchange_accounts` row with creds.

SELECT _qtss_register_key(
    'commission.sync.enabled','setup','setup','bool',
    'false'::jsonb, '',
    'Auto-refresh commission.{venue}.{side}_bps from Binance API (needs exchange_accounts creds).',
    'bool', false, 'normal', ARRAY['commission','sync']);

SELECT _qtss_register_key(
    'commission.sync.interval_hours','setup','setup','int',
    '24'::jsonb, 'hours',
    'Refresh cadence for commission_sync_loop. Min 1h, max 720h (30d).',
    'number', false, 'normal', ARRAY['commission','sync']);

SELECT _qtss_register_key(
    'commission.sync.representative_symbol','setup','setup','string',
    '"BTCUSDT"'::jsonb, '',
    'Symbol queried to read tier-aware commission rates (applies to the whole venue).',
    'string', false, 'normal', ARRAY['commission','sync']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('setup','commission.sync.enabled','false'::jsonb,'Auto-refresh commission bps — off by default.'),
    ('setup','commission.sync.interval_hours','24'::jsonb,'Refresh cadence (hours).'),
    ('setup','commission.sync.representative_symbol','"BTCUSDT"'::jsonb,'Symbol used to probe commission tiers.')
ON CONFLICT (module, config_key) DO NOTHING;
