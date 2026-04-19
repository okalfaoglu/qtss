-- 0190_chart_zigzag_l0_color_visibility.sql
--
-- Dark tema üzerinde görünürlük için L0/L1 varsayılan renkleri güncellendi:
--   L0: #546e7a (koyu slate, eriyordu) → #ffffff (beyaz)
--   L1: #42a5f5 (mavi, onay gereksinimi)  → #fbbf24 (amber/sarı)
--
-- Config catalog (`config_schema`) default'u güncellenir; henüz operator
-- override yoksa GUI otomatik yeni rengi yükler. Override varsa o kalır.
UPDATE config_schema
   SET default_value = '"#ffffff"'::jsonb,
       updated_at    = now()
 WHERE key = 'chart.zigzag.l0.color'
   AND default_value IN ('"#546e7a"'::jsonb, '"#94a3b8"'::jsonb);

UPDATE config_schema
   SET default_value = '"#fbbf24"'::jsonb,
       updated_at    = now()
 WHERE key = 'chart.zigzag.l1.color'
   AND default_value = '"#42a5f5"'::jsonb;
