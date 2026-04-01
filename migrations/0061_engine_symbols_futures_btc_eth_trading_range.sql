-- Binance USDT-M futures: Trading Range motoru (`trading_range` + `signal_dashboard`) için hedefler.
-- `signal_direction_mode = both` → LONG/SHORT (çift yönlü); spot’ta genelde `auto_segment` / long_only kalır.
-- `sort_order` düşük olan satır kline WebSocket fallback’inde önce gelir (`list_enabled_engine_symbols` → worker ilk satırdan interval/segment alır).
INSERT INTO engine_symbols (exchange, segment, symbol, interval, enabled, sort_order, label, signal_direction_mode)
VALUES
  ('binance', 'futures', 'BTCUSDT', '15m', true, -100, 'Futures TR BTC 15m', 'both'),
  ('binance', 'futures', 'ETHUSDT', '15m', true, -99, 'Futures TR ETH 15m', 'both')
ON CONFLICT (exchange, segment, symbol, interval) DO UPDATE SET
  enabled = EXCLUDED.enabled,
  sort_order = EXCLUDED.sort_order,
  label = COALESCE(EXCLUDED.label, engine_symbols.label),
  signal_direction_mode = EXCLUDED.signal_direction_mode,
  updated_at = now();
