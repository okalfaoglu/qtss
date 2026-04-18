-- 0167_faz_10_render_palette.sql
--
-- Aşama 5.C — overlay renk paleti system_config'e taşınıyor. Önceden
-- Chart.tsx'te hardcoded `FAMILY_COLORS` map'i vardı; CLAUDE.md #2'ye
-- uyum için her family + render_style anahtarı DB'den okunabilir
-- olmalı. Frontend varsayılanları hala kod içinde (bootstrap fallback)
-- ama Config Editor'dan canlı override edilebiliyor.
--
-- Anahtar şeması:
--   ui.chart.palette.family.{family}          → "#rrggbb"
--   ui.chart.palette.style.{style_key}        → "#rrggbb"  (opsiyonel)
--
-- Idempotent.

INSERT INTO system_config (module, config_key, value, description) VALUES
  ('ui', 'chart.palette.family.elliott',   '"#7dd3fc"'::jsonb,
   'Elliott ailesi chart overlay rengi (sky-300).'),
  ('ui', 'chart.palette.family.harmonic',  '"#f472b6"'::jsonb,
   'Harmonic ailesi chart overlay rengi (pink-400).'),
  ('ui', 'chart.palette.family.classical', '"#facc15"'::jsonb,
   'Classical ailesi chart overlay rengi (yellow-400).'),
  ('ui', 'chart.palette.family.wyckoff',   '"#a78bfa"'::jsonb,
   'Wyckoff ailesi chart overlay rengi (violet-400).'),
  ('ui', 'chart.palette.family.range',     '"#5eead4"'::jsonb,
   'Range/SMC ailesi chart overlay rengi (teal-300).'),
  ('ui', 'chart.palette.family.tbm',       '"#fb923c"'::jsonb,
   'TBM ailesi chart overlay rengi (orange-400).'),
  ('ui', 'chart.palette.family.candle',    '"#fca5a5"'::jsonb,
   'Candlestick ailesi chart overlay rengi (red-300).'),
  ('ui', 'chart.palette.family.gap',       '"#38bdf8"'::jsonb,
   'Gap ailesi chart overlay rengi (sky-400).'),
  ('ui', 'chart.palette.family.custom',    '"#d4d4d8"'::jsonb,
   'Bilinmeyen aile için fallback rengi (zinc-300).'),
  -- Style anahtarları — detector `render_style` döndürdüğünde bu
  -- haritadan okunur, aile renginin üzerine yazılır. İlk seed boş
  -- (detector'lar henüz style key üretmiyor). Yeni variant eklemek =
  -- bir satır INSERT, kod değişikliği yok.
  ('ui', 'chart.palette.style.bull',       '"#22c55e"'::jsonb,
   'Yönsel "bull" stil override (green-500). Detector "<family>_bull" tarzı style_key verdiğinde kullanılır.'),
  ('ui', 'chart.palette.style.bear',       '"#ef4444"'::jsonb,
   'Yönsel "bear" stil override (red-500). Detector "<family>_bear" tarzı style_key verdiğinde kullanılır.')
ON CONFLICT (module, config_key) DO NOTHING;
