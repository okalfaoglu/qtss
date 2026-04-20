-- 0198_symbol_intel_api_keys.sql
--
-- Faz 14.A — `symbol_intel` için dış veri sağlayıcı API key'leri.
-- CLAUDE.md #2 gereği `.env` yerine `config_schema`'da tutuluyor;
-- GUI Config Editor'den güvenli biçimde set edilebilir.
--
-- `sensitivity='high'` → audit log maskeler, UI'de `••••••` gösterir.

BEGIN;

INSERT INTO config_schema (
    key, category, subcategory, value_type, default_value,
    unit, description, ui_widget, requires_restart, sensitivity,
    introduced_in, tags
) VALUES
(
    'symbol_intel.coingecko.api_key',
    'symbol_intel', 'credentials', 'string', '""'::jsonb,
    NULL,
    'CoinGecko Demo/Pro API key (x-cg-demo-api-key veya x-cg-pro-api-key header).',
    'password', false, 'high', '0198',
    ARRAY['faz14','symbol_intel','coingecko']::TEXT[]
),
(
    'symbol_intel.coingecko.tier',
    'symbol_intel', 'credentials', 'string', '"demo"'::jsonb,
    NULL,
    'CoinGecko plan tier. "demo" (free) → x-cg-demo-api-key header, "pro" → x-cg-pro-api-key.',
    'select', false, 'normal', '0198',
    ARRAY['faz14','symbol_intel','coingecko']::TEXT[]
),
(
    'symbol_intel.coinmarketcap.api_key',
    'symbol_intel', 'credentials', 'string', '""'::jsonb,
    NULL,
    'CoinMarketCap Basic/Pro API key (X-CMC_PRO_API_KEY header). Faz 14.B NASDAQ/equity için.',
    'password', false, 'high', '0198',
    ARRAY['faz14','symbol_intel','coinmarketcap']::TEXT[]
)
ON CONFLICT (key) DO NOTHING;

COMMIT;
