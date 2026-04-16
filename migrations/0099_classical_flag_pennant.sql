-- 0099_classical_flag_pennant.sql
--
-- P5.2 — Bull/Bear Flag + Pennant detector config keys.
-- Flagpole: 3–20 bar içinde ≥ N×ATR yönlü hareket.
-- Flag body: paralel ters-eğimli kanal, retrace < pole×%50.
-- Pennant: küçük simetrik üçgen, yükseklik < pole×%40.
-- Reference: Bulkowski Encyclopedia, Edwards & Magee.

SELECT _qtss_register_key(
    'classical.flag_pole_min_move_atr','classical','detection','float',
    '3.0'::jsonb, 'atr_mult',
    'Flagpole minimum yönlü hareket, ATR çarpanı. Hareket ≥ N × ATR olmalı. Default 3.0.',
    'number', true, 'normal', ARRAY['classical','flag','pennant']);

SELECT _qtss_register_key(
    'classical.flag_pole_max_bars','classical','detection','integer',
    '20'::jsonb, 'bars',
    'Flagpole için geriye bakılacak maksimum bar penceresi. Default 20.',
    'number', true, 'normal', ARRAY['classical','flag','pennant']);

SELECT _qtss_register_key(
    'classical.flag_max_retrace_pct','classical','detection','float',
    '0.5'::jsonb, 'fraction',
    'Flag gövdesi yüksekliğinin flagpole''a oranı üst sınırı. Bulkowski: < %50. Default 0.5.',
    'number', true, 'normal', ARRAY['classical','flag']);

SELECT _qtss_register_key(
    'classical.flag_atr_period','classical','detection','integer',
    '14'::jsonb, 'bars',
    'Flag/Pennant için Wilder ATR periyodu. Default 14.',
    'number', true, 'normal', ARRAY['classical','flag','pennant']);

SELECT _qtss_register_key(
    'classical.flag_parallelism_tol','classical','detection','float',
    '0.3'::jsonb, 'fraction',
    'Flag kanal paralellik toleransı; |upper.slope - lower.slope|/avg. Default 0.3.',
    'number', true, 'normal', ARRAY['classical','flag']);

SELECT _qtss_register_key(
    'classical.pennant_max_height_pct_of_pole','classical','detection','float',
    '0.4'::jsonb, 'fraction',
    'Pennant gövdesinin flagpole''a maksimum oranı. Default 0.4.',
    'number', true, 'normal', ARRAY['classical','pennant']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection','classical.flag_pole_min_move_atr','3.0'::jsonb,'Flagpole min ATR-scaled move.'),
    ('detection','classical.flag_pole_max_bars','20'::jsonb,'Flagpole lookback bars.'),
    ('detection','classical.flag_max_retrace_pct','0.5'::jsonb,'Flag body max retrace fraction of pole.'),
    ('detection','classical.flag_atr_period','14'::jsonb,'ATR period for flag/pennant.'),
    ('detection','classical.flag_parallelism_tol','0.3'::jsonb,'Flag channel parallelism tolerance.'),
    ('detection','classical.pennant_max_height_pct_of_pole','0.4'::jsonb,'Pennant body height max fraction of pole.')
ON CONFLICT (module, config_key) DO NOTHING;
