-- 0194_pivot_reversal_config_seed.sql
--
-- Faz 13 — Pivot-Tabanlı Major Dip/Tepe Tespit Sistemi.
--
-- Tier (reactive L0/L1 vs major L2/L3) × Event (CHoCH/BOS/Neutral)
-- skor tablosunu, prominence floor'larını ve tier-bazlı TP/SL
-- overrides'ı `config_schema`'ya seed eder (CLAUDE.md #2 — kod
-- seviyesinde sabit yok).
--
-- Bkz: docs/FAZ_13_PIVOT_REVERSAL_MAJOR_DIP_TEPE.md

BEGIN;

-- Helper (0016'daki ile aynı imza). Migration numarasını günceller.
CREATE OR REPLACE FUNCTION _qtss_register_key(
    p_key             TEXT,
    p_category        TEXT,
    p_subcategory     TEXT,
    p_value_type      TEXT,
    p_default         JSONB,
    p_unit            TEXT,
    p_description     TEXT,
    p_ui_widget       TEXT,
    p_requires_restart BOOLEAN,
    p_sensitivity     TEXT,
    p_tags            TEXT[]
) RETURNS VOID AS $$
BEGIN
    INSERT INTO config_schema (
        key, category, subcategory, value_type, default_value,
        unit, description, ui_widget, requires_restart, sensitivity,
        introduced_in, tags
    ) VALUES (
        p_key, p_category, p_subcategory, p_value_type, p_default,
        p_unit, p_description, p_ui_widget, p_requires_restart, p_sensitivity,
        '0194', p_tags
    )
    ON CONFLICT (key) DO NOTHING;
END;
$$ LANGUAGE plpgsql;

-- ── Tier → level eşlemesi ────────────────────────────────────────
SELECT _qtss_register_key('pivot_reversal.tier.reactive.levels', 'pivot_reversal', 'tier', 'array',
    '["L0","L1"]'::jsonb, NULL,
    'Tepki (reactive) dip-tepe sayılan pivot seviyeleri.',
    'json_editor', false, 'normal', ARRAY['pivot_reversal','tier']);

SELECT _qtss_register_key('pivot_reversal.tier.major.levels',    'pivot_reversal', 'tier', 'array',
    '["L2","L3"]'::jsonb, NULL,
    'Major (structural) dip-tepe sayılan pivot seviyeleri.',
    'json_editor', false, 'normal', ARRAY['pivot_reversal','tier']);

-- ── Skor tablosu — seviye × event ────────────────────────────────
-- CHoCH
SELECT _qtss_register_key('pivot_reversal.score.L0.choch',  'pivot_reversal', 'score', 'float', '0.50'::jsonb, NULL, 'L0 CHoCH tier skoru.', 'slider', false, 'normal', ARRAY['pivot_reversal','score']);
SELECT _qtss_register_key('pivot_reversal.score.L1.choch',  'pivot_reversal', 'score', 'float', '0.65'::jsonb, NULL, 'L1 CHoCH tier skoru.', 'slider', false, 'normal', ARRAY['pivot_reversal','score']);
SELECT _qtss_register_key('pivot_reversal.score.L2.choch',  'pivot_reversal', 'score', 'float', '0.85'::jsonb, NULL, 'L2 CHoCH tier skoru.', 'slider', false, 'normal', ARRAY['pivot_reversal','score']);
SELECT _qtss_register_key('pivot_reversal.score.L3.choch',  'pivot_reversal', 'score', 'float', '0.95'::jsonb, NULL, 'L3 CHoCH tier skoru (en yüksek öncelik).', 'slider', false, 'normal', ARRAY['pivot_reversal','score']);

-- BOS
SELECT _qtss_register_key('pivot_reversal.score.L0.bos',    'pivot_reversal', 'score', 'float', '0.30'::jsonb, NULL, 'L0 BOS tier skoru.', 'slider', false, 'normal', ARRAY['pivot_reversal','score']);
SELECT _qtss_register_key('pivot_reversal.score.L1.bos',    'pivot_reversal', 'score', 'float', '0.40'::jsonb, NULL, 'L1 BOS tier skoru.', 'slider', false, 'normal', ARRAY['pivot_reversal','score']);
SELECT _qtss_register_key('pivot_reversal.score.L2.bos',    'pivot_reversal', 'score', 'float', '0.60'::jsonb, NULL, 'L2 BOS tier skoru.', 'slider', false, 'normal', ARRAY['pivot_reversal','score']);
SELECT _qtss_register_key('pivot_reversal.score.L3.bos',    'pivot_reversal', 'score', 'float', '0.70'::jsonb, NULL, 'L3 BOS tier skoru.', 'slider', false, 'normal', ARRAY['pivot_reversal','score']);

-- Neutral
SELECT _qtss_register_key('pivot_reversal.score.L0.neutral','pivot_reversal', 'score', 'float', '0.15'::jsonb, NULL, 'L0 neutral tier skoru.', 'slider', false, 'normal', ARRAY['pivot_reversal','score']);
SELECT _qtss_register_key('pivot_reversal.score.L1.neutral','pivot_reversal', 'score', 'float', '0.20'::jsonb, NULL, 'L1 neutral tier skoru.', 'slider', false, 'normal', ARRAY['pivot_reversal','score']);
SELECT _qtss_register_key('pivot_reversal.score.L2.neutral','pivot_reversal', 'score', 'float', '0.30'::jsonb, NULL, 'L2 neutral tier skoru.', 'slider', false, 'normal', ARRAY['pivot_reversal','score']);
SELECT _qtss_register_key('pivot_reversal.score.L3.neutral','pivot_reversal', 'score', 'float', '0.40'::jsonb, NULL, 'L3 neutral tier skoru.', 'slider', false, 'normal', ARRAY['pivot_reversal','score']);

-- ── Prominence floor (per level) ─────────────────────────────────
SELECT _qtss_register_key('pivot_reversal.prominence_floor.L0', 'pivot_reversal', 'filter', 'float', '0.0'::jsonb, 'atr', 'L0 minimum prominence.', 'number', false, 'normal', ARRAY['pivot_reversal','filter']);
SELECT _qtss_register_key('pivot_reversal.prominence_floor.L1', 'pivot_reversal', 'filter', 'float', '0.0'::jsonb, 'atr', 'L1 minimum prominence.', 'number', false, 'normal', ARRAY['pivot_reversal','filter']);
SELECT _qtss_register_key('pivot_reversal.prominence_floor.L2', 'pivot_reversal', 'filter', 'float', '0.0'::jsonb, 'atr', 'L2 minimum prominence.', 'number', false, 'normal', ARRAY['pivot_reversal','filter']);
SELECT _qtss_register_key('pivot_reversal.prominence_floor.L3', 'pivot_reversal', 'filter', 'float', '0.0'::jsonb, 'atr', 'L3 minimum prominence.', 'number', false, 'normal', ARRAY['pivot_reversal','filter']);

-- ── Outcome-eval tier-bazlı TP/SL overrides ──────────────────────
SELECT _qtss_register_key('eval.pivot_reversal.reactive.tp1_r',            'eval', 'pivot_reversal', 'float', '1.0'::jsonb, 'R', 'Reactive (L0/L1) TP1 R-katsayısı.', 'number', false, 'normal', ARRAY['eval','pivot_reversal']);
SELECT _qtss_register_key('eval.pivot_reversal.reactive.tp2_r',            'eval', 'pivot_reversal', 'float', '2.0'::jsonb, 'R', 'Reactive (L0/L1) TP2 R-katsayısı.', 'number', false, 'normal', ARRAY['eval','pivot_reversal']);
SELECT _qtss_register_key('eval.pivot_reversal.reactive.expiry_bars_mult', 'eval', 'pivot_reversal', 'int',   '20'::jsonb,  'bars','Reactive expiry bar çarpanı.',    'number', false, 'normal', ARRAY['eval','pivot_reversal']);
SELECT _qtss_register_key('eval.pivot_reversal.major.tp1_r',               'eval', 'pivot_reversal', 'float', '1.5'::jsonb, 'R', 'Major (L2/L3) TP1 R-katsayısı.',    'number', false, 'normal', ARRAY['eval','pivot_reversal']);
SELECT _qtss_register_key('eval.pivot_reversal.major.tp2_r',               'eval', 'pivot_reversal', 'float', '3.0'::jsonb, 'R', 'Major (L2/L3) TP2 R-katsayısı.',    'number', false, 'normal', ARRAY['eval','pivot_reversal']);
SELECT _qtss_register_key('eval.pivot_reversal.major.expiry_bars_mult',    'eval', 'pivot_reversal', 'int',   '60'::jsonb,  'bars','Major expiry bar çarpanı.',        'number', false, 'normal', ARRAY['eval','pivot_reversal']);

COMMIT;
