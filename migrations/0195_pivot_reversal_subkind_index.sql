-- 0195_pivot_reversal_subkind_index.sql
--
-- Faz 13 — Rapor sayfasında tier/event/direction filtrelerinin hızlı
-- çalışması için `qtss_v2_detections(family, pivot_level, subkind)`
-- üzerine partial index. `pivot_reversal` aile hacimli olabileceği
-- için ayrı bir partial index daha — sadece backtest modunda sweep
-- sonuçlarını kapsar.

CREATE INDEX IF NOT EXISTS idx_det_pivot_reversal_backtest
    ON qtss_v2_detections (family, pivot_level, subkind, detected_at DESC)
    WHERE family = 'pivot_reversal' AND mode = 'backtest';

CREATE INDEX IF NOT EXISTS idx_det_pivot_reversal_subkind_prefix
    ON qtss_v2_detections ((split_part(subkind, '_', 1)))
    WHERE family = 'pivot_reversal';
