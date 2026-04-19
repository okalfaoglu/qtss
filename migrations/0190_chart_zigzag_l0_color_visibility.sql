-- 0190_chart_zigzag_l0_color_visibility.sql
--
-- L0 ZigZag çizgi rengi `#546e7a` (koyu slate) dark tema üzerinde
-- görünmüyordu. `#94a3b8` (slate-400) daha okunabilir — "mikro
-- gürültü" algısını korurken canlı arkaplan üzerinde seçilebiliyor.
--
-- Config catalog (`config_schema`) default'u güncellenir; henüz
-- override yoksa GUI otomatik yeni rengi yükler. Operator Config
-- Editor'den tekrar override ederse onun değeri geçerli kalır.
UPDATE config_schema
   SET default_value = '"#94a3b8"'::jsonb,
       updated_at    = now()
 WHERE key = 'chart.zigzag.l0.color'
   AND default_value = '"#546e7a"'::jsonb;
