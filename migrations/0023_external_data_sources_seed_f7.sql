-- F7 örnek kaynaklar (Coinglass anahtarı / URL’ler ortamınıza göre güncellenmeli). `ON CONFLICT` ile idempotent.

INSERT INTO external_data_sources (key, enabled, method, url, headers_json, body_json, tick_secs, description)
VALUES
(
    'coinglass_netflow_btc',
    false,
    'GET',
    'https://open-api.coinglass.com/public/v2/exchange/netflow?symbol=BTC&ex=Binance',
    '{}'::jsonb,
    NULL,
    300,
    'BTC Binance netflow proxy — Coinglass API key: headers_json (PLAN §2).'
),
(
    'binance_taker_btcusdt',
    true,
    'GET',
    'https://fapi.binance.com/futures/data/takerlongshortRatio?symbol=BTCUSDT&period=5m&limit=10',
    '{}'::jsonb,
    NULL,
    60,
    'BTCUSDT taker long/short ratio (public FAPI).'
),
(
    'coinglass_exchange_balance_btc',
    false,
    'GET',
    'https://open-api.coinglass.com/public/v2/exchange/balance?symbol=BTC',
    '{}'::jsonb,
    NULL,
    300,
    'BTC multi-exchange balance proxy — Coinglass key gerekir.'
)
ON CONFLICT (key) DO NOTHING;
