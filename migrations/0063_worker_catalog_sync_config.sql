-- Binance katalog senkronu (worker): `instruments` + delist sonrası `is_trading=false`

INSERT INTO system_config (module, config_key, value, description, is_secret)
VALUES
  (
    'worker',
    'catalog_sync_enabled',
    '{"enabled": true}',
    'Enable periodic Binance exchangeInfo → exchanges/markets/instruments sync.',
    false
  ),
  (
    'worker',
    'catalog_sync_tick_secs',
    '{"secs": 3600}',
    'Seconds between Binance catalog sync runs (minimum enforced in code).',
    false
  )
ON CONFLICT (module, config_key) DO NOTHING;
