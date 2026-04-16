-- 0097_classical_hs_slope_time.sql
--
-- P3 — H&S / Inverse H&S: neckline slope cap + shoulder time-symmetry.
-- Bulkowski & Edwards/Magee: the neckline is nearly horizontal (~±5°)
-- and shoulders are roughly equal in time. Pre-P3 the detector only
-- required price-equal shoulders and neckline flatness scores; nothing
-- rejected steep necklines or lopsided time spans.

SELECT _qtss_register_key(
    'classical.hs_max_neckline_slope_pct','classical','detection','float',
    '0.003'::jsonb, 'fraction',
    'H&S neckline slope cap, as fraction of neckline midpoint per bar. 0.003 = 0.3%/bar ≈ ±5° on typical charts. Pattern rejected if exceeded.',
    'number', true, 'normal', ARRAY['classical','h&s']);

SELECT _qtss_register_key(
    'classical.hs_time_symmetry_tol','classical','detection','float',
    '0.5'::jsonb, 'fraction',
    'H&S shoulder time-symmetry tolerance: |LS→H bars - H→RS bars| / avg. 0.5 = ±50%; pattern rejected above this.',
    'number', true, 'normal', ARRAY['classical','h&s']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection','classical.hs_max_neckline_slope_pct','0.003'::jsonb,'H&S neckline max slope per bar (fraction).'),
    ('detection','classical.hs_time_symmetry_tol','0.5'::jsonb,'H&S shoulder time-symmetry tolerance.')
ON CONFLICT (module, config_key) DO NOTHING;
