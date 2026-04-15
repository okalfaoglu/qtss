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
    'wyckoff.sos_min_bar_width_atr_mult', 'wyckoff', 'event', 'number',
    '1.5'::jsonb, 'x_atr',
    'SOS/SOW bar range must be at least this multiple of ATR proxy. Villahermosa: wide-range bar is the single hard numeric rule.',
    'number', true, 'normal', ARRAY['wyckoff','phase_d']);

SELECT _qtss_register_key(
    'wyckoff.sos_close_third_threshold', 'wyckoff', 'event', 'number',
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

-- Operator-facing defaults mirrored into system_config for GUI editing.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detector', 'wyckoff.sos_min_bar_width_atr_mult', '1.5'::jsonb,
     'SOS/SOW wide-range gate (ATR proxy multiplier).'),
    ('detector', 'wyckoff.sos_close_third_threshold', '0.66'::jsonb,
     'SOS/SOW close-in-third gate (upper/lower fraction).'),
    ('detector', 'wyckoff.phase_b_min_bars', '10'::jsonb,
     'Minimum bars in Phase B before C may open.'),
    ('detector', 'wyckoff.phase_b_min_inner_tests', '1'::jsonb,
     'Minimum Phase-B inner tests (UA/STB/ST) before C.')
ON CONFLICT (module, config_key) DO NOTHING;
