-- 0090_wyckoff_pdf_fidelity.sql
--
-- Faz 10 / P2 — Wyckoff detector PDF fidelity patches.
--
-- Villahermosa "Wyckoff 2.0" spec uncovered multiple gaps in the live
-- detector: SOS/SOW pivots above creek were accepted without the
-- mandatory wide-range + close-in-third bar shape, and Phase B could be
-- skipped by a bare Spring firing (b_count≥2 OR has_c_event — the OR
-- was the bug). This migration seeds the new config keys; the code fix
-- lives in commits c762ddf (P2a SOS/SOW bar-shape) and this patch set
-- (P2c Phase B gate).
--
-- TTL / schematic-flip keys seeded in 0089 are untouched.

-- --- P2a: SOS/SOW bar-shape (Villahermosa ch. 7) ---
SELECT _qtss_register_key(
    'wyckoff.sos_min_bar_width_atr_mult', 'wyckoff', 'event', 'float',
    '1.5'::jsonb, 'x_atr',
    'SOS/SOW bar range must be at least this multiple of ATR proxy. Villahermosa: wide-range bar is the single hard numeric rule.',
    'number', true, 'normal', ARRAY['wyckoff','phase_d']);

SELECT _qtss_register_key(
    'wyckoff.sos_close_third_threshold', 'wyckoff', 'event', 'float',
    '0.66'::jsonb, 'fraction',
    'SOS close must sit in upper (1 - N) of its bar range (>=0.66 = upper third). Mirror for SOW lower third.',
    'number', true, 'normal', ARRAY['wyckoff','phase_d']);

-- --- P2c: Phase B real gate (no more B skipping) ---
SELECT _qtss_register_key(
    'wyckoff.phase_b_min_bars', 'wyckoff', 'phase', 'int',
    '10'::jsonb, 'bars',
    'Minimum bars between last Phase-A event and earliest Phase-C transition. Phase B is the longest phase per Wyckoff canon; cannot be skipped.',
    'number', true, 'normal', ARRAY['wyckoff','phase_b']);

SELECT _qtss_register_key(
    'wyckoff.phase_b_min_inner_tests', 'wyckoff', 'phase', 'int',
    '1'::jsonb, 'count',
    'Phase B requires at least this many internal tests (UA, ST-B, ST) beyond the Phase-A triple before C may open.',
    'number', true, 'normal', ARRAY['wyckoff','phase_b']);

-- --- P2d: Spring Test / UTAD Test (Phase C confirmation) ---
SELECT _qtss_register_key(
    'wyckoff.spring_test_max_vol_ratio', 'wyckoff', 'event', 'float',
    '0.6'::jsonb, 'fraction',
    'Spring/UTAD Test volume must be <= N * parent Spring/UTAD volume. Low-volume retest confirms Phase C.',
    'number', true, 'normal', ARRAY['wyckoff','phase_c']);

SELECT _qtss_register_key(
    'wyckoff.spring_test_window_bars', 'wyckoff', 'event', 'int',
    '8'::jsonb, 'bars',
    'Spring/UTAD Test must fire within this many bars of the parent Spring/UTAD.',
    'number', true, 'normal', ARRAY['wyckoff','phase_c']);

SELECT _qtss_register_key(
    'wyckoff.spring_test_max_distance', 'wyckoff', 'event', 'float',
    '0.10'::jsonb, 'fraction',
    'Spring/UTAD Test low must sit within N * range_height of the parent Spring/UTAD price.',
    'number', true, 'normal', ARRAY['wyckoff','phase_c']);

-- --- P5: JAC body ratio + mid-range climactic flip ---
SELECT _qtss_register_key(
    'wyckoff.jac_min_body_ratio', 'wyckoff', 'event', 'float',
    '0.5'::jsonb, 'fraction',
    'JAC body (|close-open|) must be >= N * bar_range. Tiny wicks above creek do not count as Jump-Across-Creek.',
    'number', true, 'normal', ARRAY['wyckoff','phase_d']);

SELECT _qtss_register_key(
    'wyckoff.phase_b_climactic_vol_flip_mult', 'wyckoff', 'phase', 'float',
    '3.0'::jsonb, 'x_avg',
    'Phase-B bar with volume >= N * avg flips schematic to Distribution (Villahermosa ch.5). Set 0 to disable.',
    'number', true, 'normal', ARRAY['wyckoff','phase_b']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detector', 'wyckoff.jac_min_body_ratio', '0.5'::jsonb,
     'JAC minimum body/range ratio.'),
    ('detector', 'wyckoff.phase_b_climactic_vol_flip_mult', '3.0'::jsonb,
     'Phase-B climactic volume flip multiplier (0 = disabled).')
ON CONFLICT (module, config_key) DO NOTHING;

-- --- P2-P1-#5 / #11: Spring penetration + SC bar-width ---
SELECT _qtss_register_key(
    'wyckoff.shakeout_max_penetration', 'wyckoff', 'event', 'float',
    '0.30'::jsonb, 'fraction',
    'Shakeout max penetration (deeper than ordinary Spring but bounded). Past this, the move is a true breakout.',
    'number', true, 'normal', ARRAY['wyckoff','phase_c']);

-- Override prior 0.30 default — ordinary Springs only pierce <=12% now.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection', 'wyckoff.max_penetration', '0.12'::jsonb,
     'Ordinary Spring/UTAD max penetration (fraction of range height). Tightened from 0.30 per Villahermosa ch. 6.'),
    ('detection', 'wyckoff.shakeout_max_penetration', '0.30'::jsonb,
     'Shakeout (aggressive Spring variant) max penetration.')
ON CONFLICT (module, config_key) DO UPDATE SET value = EXCLUDED.value;

-- Operator-facing defaults mirrored into system_config for GUI editing.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detector', 'wyckoff.sos_min_bar_width_atr_mult', '1.5'::jsonb,
     'SOS/SOW wide-range gate (ATR proxy multiplier).'),
    ('detector', 'wyckoff.sos_close_third_threshold', '0.66'::jsonb,
     'SOS/SOW close-in-third gate (upper/lower fraction).'),
    ('detector', 'wyckoff.phase_b_min_bars', '10'::jsonb,
     'Minimum bars in Phase B before C may open.'),
    ('detector', 'wyckoff.phase_b_min_inner_tests', '1'::jsonb,
     'Minimum Phase-B inner tests (UA/STB/ST) before C.'),
    ('detector', 'wyckoff.spring_test_max_vol_ratio', '0.6'::jsonb,
     'Spring/UTAD Test volume fraction of parent Spring/UTAD.'),
    ('detector', 'wyckoff.spring_test_window_bars', '8'::jsonb,
     'Bars within which Spring/UTAD Test must follow parent.'),
    ('detector', 'wyckoff.spring_test_max_distance', '0.10'::jsonb,
     'Spring/UTAD Test price distance cap (fraction of range height).')
ON CONFLICT (module, config_key) DO NOTHING;

-- --- P6: A→B strict toggle (#17) + per-phase dwell (#15) ---
SELECT _qtss_register_key(
    'wyckoff.phase.a_to_b.require_st', 'wyckoff', 'phase', 'bool',
    'false'::jsonb, 'flag',
    'When true, A→B requires explicit ST in addition to climax+AR. Default false (canonical relaxed).',
    'bool', true, 'normal', ARRAY['wyckoff','phase_a']);

SELECT _qtss_register_key(
    'wyckoff.phase.min_dwell_bars', 'wyckoff', 'phase', 'int',
    '3'::jsonb, 'bars',
    'Minimum bars the tracker must spend in the current phase before advancing. Prevents sub-bar phase sprints.',
    'number', true, 'normal', ARRAY['wyckoff','phases']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detector', 'wyckoff.phase.a_to_b.require_st', 'false'::jsonb,
     'Strict A→B gate: require explicit ST.'),
    ('detector', 'wyckoff.phase.min_dwell_bars', '3'::jsonb,
     'Min bars in current phase before next transition fires.')
ON CONFLICT (module, config_key) DO NOTHING;
