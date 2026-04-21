-- 0199_outcome_maturity.sql
--
-- Fix C — pivot_reversal outcomes'a "maturity" kolonu.
-- `immature`: pivot fiyatı ilk N bar içinde kırıldı → "tepki dibi",
--  gerçek bir major değil. Win-rate hesabında dışlanır.
-- `mature`:  pivot held, trade normal sonuçlandı.
-- Diğer aileler için NULL kalır (kavram sadece pivot_reversal'e ait).

BEGIN;

ALTER TABLE qtss_v2_detection_outcomes
    ADD COLUMN IF NOT EXISTS maturity text;

-- Filtre sorgusunu hızlandıracak kısmi index.
CREATE INDEX IF NOT EXISTS idx_outcomes_maturity
    ON qtss_v2_detection_outcomes (maturity)
    WHERE maturity IS NOT NULL;

-- Config eşikleri — CLAUDE.md #2 gereği koda yazmıyoruz.
-- Bar sayısı, pivot onaylandıktan sonra pivot fiyatının kırılmaması
-- beklenen penceredir. Seviye yükseldikçe daha uzun bekliyoruz.
INSERT INTO config_schema (
    key, category, subcategory, value_type, default_value,
    unit, description, ui_widget, requires_restart, sensitivity,
    introduced_in, tags
) VALUES
('eval.pivot_reversal.immature_window_bars.L0', 'eval', 'pivot_reversal', 'int', '3'::jsonb,
    'bars', 'L0 pivotun "immature" sayılması için kırılma penceresi.',
    'number', false, 'normal', '0199', ARRAY['faz13','outcome']::TEXT[]),
('eval.pivot_reversal.immature_window_bars.L1', 'eval', 'pivot_reversal', 'int', '5'::jsonb,
    'bars', 'L1 immature penceresi.',
    'number', false, 'normal', '0199', ARRAY['faz13','outcome']::TEXT[]),
('eval.pivot_reversal.immature_window_bars.L2', 'eval', 'pivot_reversal', 'int', '8'::jsonb,
    'bars', 'L2 immature penceresi.',
    'number', false, 'normal', '0199', ARRAY['faz13','outcome']::TEXT[]),
('eval.pivot_reversal.immature_window_bars.L3', 'eval', 'pivot_reversal', 'int', '12'::jsonb,
    'bars', 'L3 immature penceresi.',
    'number', false, 'normal', '0199', ARRAY['faz13','outcome']::TEXT[])
ON CONFLICT (key) DO NOTHING;

COMMIT;
