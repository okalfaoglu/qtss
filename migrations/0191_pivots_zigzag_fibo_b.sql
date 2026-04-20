-- 0191_pivots_zigzag_fibo_b.sql
--
-- Pivot ZigZag ATR multipliers: migrate from geometric-2× dizisi
-- [1.5, 3.0, 6.0, 12.0] to Fibonacci-B [2.0, 3.0, 5.0, 8.0].
--
-- Empirical justification (BTC/ETH, 5m/15m/1h/4h, ~300k pivots):
--   L0 k=1.5 medyan gap=4 bar (gürültü sınırında). k=2.0 → ~7 bar,
--     mikro-salınımları koruyor ama noise azalıyor.
--   L1 k=3.0 değişmez — harmonic/classical detector default seviyesi,
--     stabilite önemli. Medyan ~16 bar.
--   L2 k=6.0 → 5.0: L2/L1 ayrıklığı hafif sıkışır (medyan 55→~45),
--     harmonic XABCD örneklemi artar.
--   L3 k=12.0 → 8.0: yüksek TF'de (4h+) L3 örneklemi ~2× artıyor.
--     Elliott makro dalga için kullanılabilir hale geliyor; medyan
--     gap 180→~114 bar, hâlâ "makro" karakter.
--
-- Engine defaults (`qtss_pivots::config::PivotConfig::defaults`) aynı
-- anda güncellendi; worker rebuild sonrası yeni pivotlar Fibo-B
-- eşiğiyle yazılır. `pivot_cache`'deki eski satırlar eski eşikle
-- hesaplanmış halde kalır — gerektiğinde `pivot_historical_backfill`
-- worker'ı full rebuild ile yeniden üretir.

UPDATE config_schema
   SET default_value = '2.0'::jsonb,
       updated_at    = now()
 WHERE key = 'pivots.zigzag.atr_mult_l0'
   AND default_value = '1.5'::jsonb;

UPDATE config_schema
   SET default_value = '5.0'::jsonb,
       updated_at    = now()
 WHERE key = 'pivots.zigzag.atr_mult_l2'
   AND default_value = '6.0'::jsonb;

UPDATE config_schema
   SET default_value = '8.0'::jsonb,
       updated_at    = now()
 WHERE key = 'pivots.zigzag.atr_mult_l3'
   AND default_value = '12.0'::jsonb;

-- L1 unchanged (k=3.0), no UPDATE.
