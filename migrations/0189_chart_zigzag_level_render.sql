-- 0189_chart_zigzag_level_render.sql
--
-- Chart GUI — per-level ZigZag render config.
--
-- Backend `qtss-pivots` crate produces four cascaded ZigZag pivot
-- levels (L0 micro … L3 macro) and writes them to `pivot_cache`.
-- The chart overlay now renders each level as its own line series so
-- the operator can toggle L0/L1/L2/L3 independently in the GUI.
--
-- Colors / widths / styles are NOT hardcoded in the frontend (CLAUDE.md
-- #2). Defaults seeded here; operator can edit live via Config Editor.

-- Visibility defaults (operator can override per-session in the GUI;
-- these are the initial load-time values).
SELECT _qtss_register_key(
    'chart.zigzag.l0.enabled', 'chart', 'zigzag',
    'bool', 'false'::jsonb, '',
    'L0 (mikro) ZigZag pivot overlay default açık mı. GUI''da toggle ile anlık değiştirilebilir.',
    'toggle', false, 'normal', ARRAY['chart','zigzag','pivot_level']
);
SELECT _qtss_register_key(
    'chart.zigzag.l1.enabled', 'chart', 'zigzag',
    'bool', 'true'::jsonb, '',
    'L1 ZigZag pivot overlay default açık mı. Harmonic/Classical detectors''un varsayılan seviyesi.',
    'toggle', false, 'normal', ARRAY['chart','zigzag','pivot_level']
);
SELECT _qtss_register_key(
    'chart.zigzag.l2.enabled', 'chart', 'zigzag',
    'bool', 'false'::jsonb, '',
    'L2 (ara) ZigZag pivot overlay default açık mı.',
    'toggle', false, 'normal', ARRAY['chart','zigzag','pivot_level']
);
SELECT _qtss_register_key(
    'chart.zigzag.l3.enabled', 'chart', 'zigzag',
    'bool', 'false'::jsonb, '',
    'L3 (makro / Elliott ana dalga) ZigZag pivot overlay default açık mı.',
    'toggle', false, 'normal', ARRAY['chart','zigzag','pivot_level']
);

-- Colors — one per level. Hex, dark-theme tuned.
SELECT _qtss_register_key(
    'chart.zigzag.l0.color', 'chart', 'zigzag',
    'string', '"#ffffff"'::jsonb, '',
    'L0 ZigZag çizgisi rengi (hex).',
    'color', false, 'normal', ARRAY['chart','zigzag','pivot_level','style']
);
SELECT _qtss_register_key(
    'chart.zigzag.l1.color', 'chart', 'zigzag',
    'string', '"#fbbf24"'::jsonb, '',
    'L1 ZigZag çizgisi rengi (hex).',
    'color', false, 'normal', ARRAY['chart','zigzag','pivot_level','style']
);
SELECT _qtss_register_key(
    'chart.zigzag.l2.color', 'chart', 'zigzag',
    'string', '"#ab47bc"'::jsonb, '',
    'L2 ZigZag çizgisi rengi (hex).',
    'color', false, 'normal', ARRAY['chart','zigzag','pivot_level','style']
);
SELECT _qtss_register_key(
    'chart.zigzag.l3.color', 'chart', 'zigzag',
    'string', '"#ff7043"'::jsonb, '',
    'L3 ZigZag çizgisi rengi (hex).',
    'color', false, 'normal', ARRAY['chart','zigzag','pivot_level','style']
);

-- Widths (px) — daha derin seviye daha kalın.
SELECT _qtss_register_key(
    'chart.zigzag.l0.width', 'chart', 'zigzag',
    'int', '1'::jsonb, '',
    'L0 ZigZag çizgi kalınlığı (px).',
    'number', false, 'normal', ARRAY['chart','zigzag','pivot_level','style']
);
SELECT _qtss_register_key(
    'chart.zigzag.l1.width', 'chart', 'zigzag',
    'int', '2'::jsonb, '',
    'L1 ZigZag çizgi kalınlığı (px).',
    'number', false, 'normal', ARRAY['chart','zigzag','pivot_level','style']
);
SELECT _qtss_register_key(
    'chart.zigzag.l2.width', 'chart', 'zigzag',
    'int', '2'::jsonb, '',
    'L2 ZigZag çizgi kalınlığı (px).',
    'number', false, 'normal', ARRAY['chart','zigzag','pivot_level','style']
);
SELECT _qtss_register_key(
    'chart.zigzag.l3.width', 'chart', 'zigzag',
    'int', '3'::jsonb, '',
    'L3 ZigZag çizgi kalınlığı (px).',
    'number', false, 'normal', ARRAY['chart','zigzag','pivot_level','style']
);

-- Line style: solid / dashed / dotted
SELECT _qtss_register_key(
    'chart.zigzag.l0.style', 'chart', 'zigzag',
    'string', '"dotted"'::jsonb, '',
    'L0 ZigZag çizgi stili (solid|dashed|dotted).',
    'select', false, 'normal', ARRAY['chart','zigzag','pivot_level','style']
);
SELECT _qtss_register_key(
    'chart.zigzag.l1.style', 'chart', 'zigzag',
    'string', '"solid"'::jsonb, '',
    'L1 ZigZag çizgi stili (solid|dashed|dotted).',
    'select', false, 'normal', ARRAY['chart','zigzag','pivot_level','style']
);
SELECT _qtss_register_key(
    'chart.zigzag.l2.style', 'chart', 'zigzag',
    'string', '"solid"'::jsonb, '',
    'L2 ZigZag çizgi stili (solid|dashed|dotted).',
    'select', false, 'normal', ARRAY['chart','zigzag','pivot_level','style']
);
SELECT _qtss_register_key(
    'chart.zigzag.l3.style', 'chart', 'zigzag',
    'string', '"solid"'::jsonb, '',
    'L3 ZigZag çizgi stili (solid|dashed|dotted).',
    'select', false, 'normal', ARRAY['chart','zigzag','pivot_level','style']
);

-- Pivot node sayısı limiti — API default'u; GUI override edebilir.
SELECT _qtss_register_key(
    'chart.zigzag.max_points_per_level', 'chart', 'zigzag',
    'int', '2000'::jsonb, '',
    'Bir seviyede chart''a çekilen maksimum pivot sayısı. Büyük değerler render''ı yavaşlatır.',
    'number', false, 'normal', ARRAY['chart','zigzag','limit']
);
