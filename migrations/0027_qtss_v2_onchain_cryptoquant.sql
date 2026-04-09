-- 0027_qtss_v2_onchain_cryptoquant.sql
--
-- Faz 7.7 / C2 — second Chain-category fetcher (CryptoQuant) lives
-- alongside Glassnode. Both feed `CategoryKind::Chain` so the
-- aggregator naturally averages them when both are enabled. Operators
-- can flip them independently while comparing data quality during
-- the Glassnode trial period.

SELECT _qtss_register_key('onchain.fetcher.cryptoquant.enabled', 'onchain','fetcher','bool',
    'false'::jsonb, NULL,
    'Enable CryptoQuant cohort fetcher (paid, BTC/ETH only — needs api key).',
    'toggle', true, 'normal', ARRAY['onchain','fetcher']);

SELECT _qtss_register_key('onchain.fetcher.cryptoquant.api_key', 'onchain','fetcher','string',
    '""'::jsonb, NULL,
    'CryptoQuant API key. Empty disables the fetcher even when its enabled flag is true.',
    'text', true, 'high', ARRAY['onchain','fetcher','secret']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('onchain', 'fetcher.cryptoquant.enabled', 'false'::jsonb, 'Enable CryptoQuant cohort fetcher.'),
    ('onchain', 'fetcher.cryptoquant.api_key', '""'::jsonb,    'CryptoQuant API key (empty disables).')
ON CONFLICT (module, config_key) DO NOTHING;
