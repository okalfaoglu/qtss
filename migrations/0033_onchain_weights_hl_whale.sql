-- `hl_whale` weight + optional Nansen flow-intel symbol map (QTSS_CURSOR_DEV_GUIDE).

UPDATE app_config
SET value = value || '{"hl_whale": 1.0}'::jsonb
WHERE key = 'onchain_signal_weights';

INSERT INTO app_config (key, value, description)
VALUES (
    'nansen_flow_intel_by_symbol',
    '{}'::jsonb,
    'Per-symbol JSON bodies for Nansen tgm/flow-intelligence (see worker nansen_extended)'
)
ON CONFLICT (key) DO NOTHING;
