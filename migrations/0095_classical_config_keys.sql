-- 0095_classical_config_keys.sql
--
-- P1 — Move classical detector hardcoded constants to system_config.
-- shapes.rs previously contained:
--   * 0.001  flatness threshold (%/bar) for triangle trendlines
--   * 0.3    flatness floor score
--   * 1.5    neckline tolerance multiplier (H&S)
--   * 0.5    symmetrical-triangle slope symmetry tolerance
-- Per CLAUDE.md #2 every tunable must live in system_config and be
-- editable from the Config Editor without a restart.

SELECT _qtss_register_key(
    'classical.pivot_level','classical','detection','string',
    '"L1"'::jsonb, NULL,
    'Pivot level consumed by classical detector (L0/L1/L2).',
    'enum', true, 'normal', ARRAY['classical']);

SELECT _qtss_register_key(
    'classical.min_structural_score','classical','detection','float',
    '0.50'::jsonb, 'score',
    'Drop classical candidates whose structural score falls below this floor.',
    'number', true, 'normal', ARRAY['classical']);

SELECT _qtss_register_key(
    'classical.equality_tolerance','classical','detection','float',
    '0.03'::jsonb, 'fraction',
    'Max relative deviation between two "equal" peaks (double top, shoulders). 0.03 = 3%.',
    'number', true, 'normal', ARRAY['classical']);

SELECT _qtss_register_key(
    'classical.apex_horizon_bars','classical','detection','int',
    '50'::jsonb, 'bars',
    'Triangle apex must land within this many future bars of the last pivot.',
    'number', true, 'normal', ARRAY['classical','triangle']);

SELECT _qtss_register_key(
    'classical.flatness_threshold_pct','classical','detection','float',
    '0.001'::jsonb, 'fraction',
    'Slope per bar below which a trendline is considered flat (for asc/desc triangles). 0.001 = 0.1%/bar.',
    'number', true, 'normal', ARRAY['classical','triangle']);

SELECT _qtss_register_key(
    'classical.flatness_min_score','classical','detection','float',
    '0.3'::jsonb, 'score',
    'Minimum flatness score to keep an asc/desc triangle candidate (0..1).',
    'number', true, 'normal', ARRAY['classical','triangle']);

SELECT _qtss_register_key(
    'classical.neckline_tolerance_mult','classical','detection','float',
    '1.5'::jsonb, 'multiplier',
    'H&S neckline equality tolerance = equality_tolerance * this multiplier. Typical 1.0..2.0.',
    'number', true, 'normal', ARRAY['classical','h&s']);

SELECT _qtss_register_key(
    'classical.triangle_symmetry_tol','classical','detection','float',
    '0.5'::jsonb, 'fraction',
    'Symmetrical-triangle slope symmetry tolerance: |upper.slope| vs lower.slope.',
    'number', true, 'normal', ARRAY['classical','triangle']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection','classical.pivot_level','"L1"'::jsonb, 'Pivot level for classical detector.'),
    ('detection','classical.min_structural_score','0.50'::jsonb, 'Min structural score for classical.'),
    ('detection','classical.equality_tolerance','0.03'::jsonb, 'Equality tol for classical peaks.'),
    ('detection','classical.apex_horizon_bars','50'::jsonb, 'Triangle apex horizon bars.'),
    ('detection','classical.flatness_threshold_pct','0.001'::jsonb, 'Triangle flatness threshold.'),
    ('detection','classical.flatness_min_score','0.3'::jsonb, 'Triangle flatness min score.'),
    ('detection','classical.neckline_tolerance_mult','1.5'::jsonb, 'H&S neckline tol multiplier.'),
    ('detection','classical.triangle_symmetry_tol','0.5'::jsonb, 'Symmetrical triangle sym tol.')
ON CONFLICT (module, config_key) DO NOTHING;
