-- 0100_classical_channel.sql
--
-- P5.4 — Price Channel (ascending/descending paralel kanal).
-- Rectangle'dan farkı: çizgiler eğimli (trend var). Asc → bull bias,
-- desc → bear bias.

SELECT _qtss_register_key(
    'classical.channel_parallelism_tol','classical','detection','float',
    '0.15'::jsonb, 'fraction',
    'Channel paralellik toleransı; |upper.slope - lower.slope|/avg. Default 0.15.',
    'number', true, 'normal', ARRAY['classical','channel']);

SELECT _qtss_register_key(
    'classical.channel_min_bars','classical','detection','int',
    '20'::jsonb, 'bars',
    'Channel minimum süresi (ilk-son pivot bar farkı). Default 20.',
    'number', true, 'normal', ARRAY['classical','channel']);

SELECT _qtss_register_key(
    'classical.channel_min_slope_pct','classical','detection','float',
    '0.001'::jsonb, 'fraction',
    'Channel minimum |slope| eşiği (fraction per bar). Eşiğin altında rectangle sayılır. Default 0.001 = %0.1/bar.',
    'number', true, 'normal', ARRAY['classical','channel']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection','classical.channel_parallelism_tol','0.15'::jsonb,'Channel parallelism tolerance.'),
    ('detection','classical.channel_min_bars','20'::jsonb,'Channel minimum duration in bars.'),
    ('detection','classical.channel_min_slope_pct','0.001'::jsonb,'Channel minimum slope per bar (fraction).')
ON CONFLICT (module, config_key) DO NOTHING;
