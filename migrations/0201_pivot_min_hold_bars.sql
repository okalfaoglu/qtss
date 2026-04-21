-- 0201_pivot_min_hold_bars.sql
--
-- **Fix B** — `qtss-pivots` ZigZag'ine per-level `min_hold_bars` gate
-- eklendi. Pivotun "structural" sayılabilmesi için extreme'in en az N
-- raw bar dayanması gerekir. Next-bar punch-through tepki diplerini
-- kırar (382-423/500 false-loss sorununun kök nedeni).
--
-- Defaultlar Fibonacci-B progresyonu — atr_mult ile aynı mantık:
-- seviye yükseldikçe hem threshold hem hold süresi artar.
--
-- Worker/orchestrator kodu config_schema üzerinden değeri okuyup
-- `PivotConfig.min_hold_bars` alanına basar. Şu an kod varsayılan
-- `[2, 3, 5, 8]` ile derleniyor — migration seed'i GUI Config Editor
-- üzerinden canlı override için gerekli (CLAUDE.md #2).

BEGIN;

INSERT INTO config_schema (
    key, category, subcategory, value_type, default_value,
    unit, description, ui_widget, requires_restart, sensitivity,
    introduced_in, tags
) VALUES
('pivots.zigzag.min_hold_bars.L0', 'pivots', 'zigzag', 'int', '2'::jsonb,
    'bars', 'L0 pivotunun structural sayılması için extreme''in dayanması gereken min bar. Fix B.',
    'number', true, 'high', '0201', ARRAY['faz13','pivot','fix-b']::TEXT[]),
('pivots.zigzag.min_hold_bars.L1', 'pivots', 'zigzag', 'int', '3'::jsonb,
    'bars', 'L1 pivotunun min hold bar sayısı.',
    'number', true, 'high', '0201', ARRAY['faz13','pivot','fix-b']::TEXT[]),
('pivots.zigzag.min_hold_bars.L2', 'pivots', 'zigzag', 'int', '5'::jsonb,
    'bars', 'L2 pivotunun min hold bar sayısı.',
    'number', true, 'high', '0201', ARRAY['faz13','pivot','fix-b']::TEXT[]),
('pivots.zigzag.min_hold_bars.L3', 'pivots', 'zigzag', 'int', '8'::jsonb,
    'bars', 'L3 pivotunun min hold bar sayısı.',
    'number', true, 'high', '0201', ARRAY['faz13','pivot','fix-b']::TEXT[])
ON CONFLICT (key) DO NOTHING;

COMMIT;
