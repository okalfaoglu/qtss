-- 0166_faz_10_render_geometry.sql
--
-- Aşama 5 — Migration-tabanlı overlay katmanı iskeleti.
--
-- Detectors artık kendi geometrilerini explicit olarak tanımlayabilir:
--   * render_geometry JSONB — kind + payload (polyline, horizontal_band,
--     candle_annotation, gap_marker, head_shoulders, two_lines, arc,
--     double_pattern, diamond, v_spike, fibonacci_ruler)
--   * render_style TEXT    — aile/varyant renk/stroke anahtarı (frontend
--     RENDER_KIND_REGISTRY dispatch'ine input)
--   * render_labels JSONB  — anchor/leg üstü metin notları
--
-- Tümü nullable — eski detection'lar mevcut anchor tabanlı çizimle
-- uyumlu kalır (opt-in binding). Frontend önce render_geometry'ye bakar,
-- yoksa legacy anchor path'ine düşer.
--
-- Idempotent.

ALTER TABLE qtss_v2_detections
  ADD COLUMN IF NOT EXISTS render_geometry JSONB,
  ADD COLUMN IF NOT EXISTS render_style    TEXT,
  ADD COLUMN IF NOT EXISTS render_labels   JSONB;

COMMENT ON COLUMN qtss_v2_detections.render_geometry IS
  'Aşama 5 overlay katmanı — detector''ın explicit geometri kontratı (kind+payload). NULL ise anchor-derived legacy render.';
COMMENT ON COLUMN qtss_v2_detections.render_style IS
  'Aşama 5 — aile/varyant stil anahtarı. Frontend RENDER_KIND_REGISTRY renk/stroke seçiminde kullanır.';
COMMENT ON COLUMN qtss_v2_detections.render_labels IS
  'Aşama 5 — anchor/leg üstü metin notları (ör. Elliott dalga numarası, harmonic oran etiketi).';
