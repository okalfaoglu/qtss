-- 0103_vprofile_config.sql
--
-- P7.1 — Volume Profile (qtss-vprofile) runtime config keys.
-- Auction-theory primitives: VPOC, VAH/VAL, HVN/LVN, naked VPOC.
-- Villahermosa *Wyckoff 2.0* §7.4 — volume profile is the reference
-- skeleton for SL/TP decisions in the forthcoming setup engine.

SELECT _qtss_register_key(
    'vprofile.bin_count','vprofile','detection','int',
    '50'::jsonb, 'bins',
    'Price-bin count between range low/high. Higher = finer resolution. Default 50.',
    'number', true, 'normal', ARRAY['vprofile']);

SELECT _qtss_register_key(
    'vprofile.value_area_pct','vprofile','detection','float',
    '0.70'::jsonb, 'fraction',
    'Volume fraction the Value Area must contain. Auction theory standard = 0.70.',
    'number', true, 'normal', ARRAY['vprofile']);

SELECT _qtss_register_key(
    'vprofile.hvn_min_prominence_pct','vprofile','detection','float',
    '1.30'::jsonb, 'ratio',
    'Min bin-volume / local-mean ratio to qualify as HVN. Default 1.30 (30% above mean).',
    'number', true, 'normal', ARRAY['vprofile','hvn']);

SELECT _qtss_register_key(
    'vprofile.lvn_max_pct','vprofile','detection','float',
    '0.20'::jsonb, 'ratio',
    'Max bin-volume / local-mean ratio to qualify as LVN (vacuum). Default 0.20.',
    'number', true, 'normal', ARRAY['vprofile','lvn']);

SELECT _qtss_register_key(
    'vprofile.local_neighbourhood_half_width','vprofile','detection','int',
    '3'::jsonb, 'bins',
    'Half-width (in bins) of the local window used for HVN/LVN prominence. Default 3 → 7-bin window.',
    'number', true, 'normal', ARRAY['vprofile']);

SELECT _qtss_register_key(
    'vprofile.naked_vpoc_lookback_ranges','vprofile','detection','int',
    '10'::jsonb, 'ranges',
    'How many historical profile snapshots to scan for naked VPOCs. Default 10.',
    'number', true, 'normal', ARRAY['vprofile','naked']);

SELECT _qtss_register_key(
    'vprofile.min_bars_for_profile','vprofile','detection','int',
    '20'::jsonb, 'bars',
    'Minimum bar count before a profile is considered trustworthy. Default 20.',
    'number', true, 'normal', ARRAY['vprofile']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection','vprofile.bin_count','50'::jsonb,'Volume profile bin count.'),
    ('detection','vprofile.value_area_pct','0.70'::jsonb,'Value Area fraction of total volume.'),
    ('detection','vprofile.hvn_min_prominence_pct','1.30'::jsonb,'HVN min prominence (bin / local-mean).'),
    ('detection','vprofile.lvn_max_pct','0.20'::jsonb,'LVN max ratio (bin / local-mean).'),
    ('detection','vprofile.local_neighbourhood_half_width','3'::jsonb,'Half-width of local window for HVN/LVN.'),
    ('detection','vprofile.naked_vpoc_lookback_ranges','10'::jsonb,'Naked VPOC lookback horizon (ranges).'),
    ('detection','vprofile.min_bars_for_profile','20'::jsonb,'Min bars before profile is trustworthy.')
ON CONFLICT (module, config_key) DO NOTHING;
