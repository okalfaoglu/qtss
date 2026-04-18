-- 0168_faz_10_candle_trend_mode.sql
--
-- TV parity (tr.tradingview.com/support/folders/43000570503) — her
-- candlestick pattern sayfasında trend context için 3 seçenek
-- açıklanır: SMA50, SMA50+SMA200, "tespit yok". Bizim legacy mantık
-- kümülatif yüzde getirisi (`pct`) üzerinden çalışıyordu — TV'nin
-- sayfalarında geçmiyor. Bu migration `candle.trend_mode` anahtarını
-- seed eder; kod tarafı (qtss-candles) tek noktadan dispatch ediyor.
--
-- Değerler:
--   "sma50"       → price vs SMA50
--   "sma50_200"   → price vs SMA50 ve SMA50 vs SMA200 (TV "stronger"; default)
--   "pct"         → legacy kümülatif yüzde getirisi
--   "none"        → trend guard kapalı
--
-- Yetersiz bar (SMA50 için <50, SMA200 için <200) durumunda kod
-- otomatik olarak bir alt seviyeye düşer (sma50_200 → sma50 → pct).
-- Bu yüzden yeni sembollerde bootstrap sırasında da pattern emit
-- edilmeye devam eder.
--
-- Idempotent.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('detection', 'candle.trend_mode', '"sma50_200"'::jsonb,
   'Candle prior-trend classifier: sma50 | sma50_200 | pct | none. TV parity default sma50_200.')
ON CONFLICT (module, config_key) DO NOTHING;
