-- 0101_classical_cup_rounding.sql
--
-- P5.5 — Cup & Handle (bull) + Inverse Cup & Handle (bear)
--      + Rounding Bottom (bull) + Rounding Top (bear).
-- Curvature parabolic fit R² ile doğrulanır. Reference: Bulkowski.

SELECT _qtss_register_key(
    'classical.cup_min_bars','classical','detection','integer',
    '30'::jsonb, 'bars',
    'Cup minimum süresi (rim_left → rim_right bar farkı). Default 30.',
    'number', true, 'normal', ARRAY['classical','cup']);

SELECT _qtss_register_key(
    'classical.cup_rim_equality_tol','classical','detection','float',
    '0.03'::jsonb, 'fraction',
    'Cup rim eşitliği toleransı (sol/sağ rim |diff|/mid). Default 0.03.',
    'number', true, 'normal', ARRAY['classical','cup','rounding']);

SELECT _qtss_register_key(
    'classical.cup_min_depth_pct','classical','detection','float',
    '0.12'::jsonb, 'fraction',
    'Cup minimum derinlik oranı (rim''in fraction''ı). Default 0.12.',
    'number', true, 'normal', ARRAY['classical','cup']);

SELECT _qtss_register_key(
    'classical.cup_max_depth_pct','classical','detection','float',
    '0.50'::jsonb, 'fraction',
    'Cup maksimum derinlik oranı. Default 0.50.',
    'number', true, 'normal', ARRAY['classical','cup']);

SELECT _qtss_register_key(
    'classical.cup_roundness_r2','classical','detection','float',
    '0.6'::jsonb, 'r_squared',
    'Cup parabolic fit R² eşiği (yuvarlaklık). Default 0.6.',
    'number', true, 'normal', ARRAY['classical','cup']);

SELECT _qtss_register_key(
    'classical.handle_max_depth_pct_of_cup','classical','detection','float',
    '0.5'::jsonb, 'fraction',
    'Handle derinliğinin cup derinliğine oranı üst sınırı. Default 0.5.',
    'number', true, 'normal', ARRAY['classical','cup']);

SELECT _qtss_register_key(
    'classical.rounding_min_bars','classical','detection','integer',
    '40'::jsonb, 'bars',
    'Rounding minimum süresi. Default 40.',
    'number', true, 'normal', ARRAY['classical','rounding']);

SELECT _qtss_register_key(
    'classical.rounding_roundness_r2','classical','detection','float',
    '0.65'::jsonb, 'r_squared',
    'Rounding parabolic fit R² eşiği. Default 0.65.',
    'number', true, 'normal', ARRAY['classical','rounding']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection','classical.cup_min_bars','30'::jsonb,'Cup minimum duration.'),
    ('detection','classical.cup_rim_equality_tol','0.03'::jsonb,'Cup rim equality tolerance.'),
    ('detection','classical.cup_min_depth_pct','0.12'::jsonb,'Cup min depth fraction.'),
    ('detection','classical.cup_max_depth_pct','0.50'::jsonb,'Cup max depth fraction.'),
    ('detection','classical.cup_roundness_r2','0.6'::jsonb,'Cup parabolic R² floor.'),
    ('detection','classical.handle_max_depth_pct_of_cup','0.5'::jsonb,'Handle max depth fraction of cup.'),
    ('detection','classical.rounding_min_bars','40'::jsonb,'Rounding min duration.'),
    ('detection','classical.rounding_roundness_r2','0.65'::jsonb,'Rounding parabolic R² floor.')
ON CONFLICT (module, config_key) DO NOTHING;
