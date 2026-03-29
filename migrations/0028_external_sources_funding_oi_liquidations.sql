-- Binance USDT-M public FAPI: funding + open interest (ücretsiz).
-- Coinglass likidasyon özeti — varsayılan kapalı; URL + API-Key dokümantasyona göre güncellenmeli.

INSERT INTO external_data_sources (key, enabled, method, url, headers_json, body_json, tick_secs, description)
VALUES
(
    'binance_premium_btcusdt',
    true,
    'GET',
    'https://fapi.binance.com/fapi/v1/premiumIndex?symbol=BTCUSDT',
    '{}'::jsonb,
    NULL,
    120,
    'lastFundingRate — confluence funding_oi / onchain karışımı.'
),
(
    'binance_open_interest_btcusdt',
    true,
    'GET',
    'https://fapi.binance.com/fapi/v1/openInterest?symbol=BTCUSDT',
    '{}'::jsonb,
    NULL,
    120,
    'OI seviyesi — zayıf kaldıraç ısısı (zaman serisi olmadan yön sınırlı).'
),
(
    'coinglass_liquidations_btc',
    false,
    'GET',
    'https://open-api.coinglass.com/public/v2/liquidation/info?symbol=BTC',
    '{}'::jsonb,
    NULL,
    300,
    'Likidasyon özeti — Coinglass API-Key: headers_json; endpoint sürümünü https://www.coinglass.com ile doğrulayın.'
)
ON CONFLICT (key) DO NOTHING;
