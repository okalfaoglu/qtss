-- 0102_validator_retest_channel.sql
--
-- P6 — Retest / throwback channel config keys.
-- Kırılım sonrası kırılan seviyeye geri test + sıçrama → continuation
-- onayı (kitabi: "kırılan direnç destek olur"). Failed retest = breakout
-- başarısız.

SELECT _qtss_register_key(
    'validator.classical.retest_tolerance_pct','validator','detection','float',
    '0.005'::jsonb, 'fraction',
    'Retest dokunma toleransı, fiyatın yüzdesi olarak. Default 0.005 = %0.5.',
    'number', true, 'normal', ARRAY['validator','classical','retest']);

SELECT _qtss_register_key(
    'validator.classical.retest_max_bars_after_breakout','validator','detection','int',
    '30'::jsonb, 'bars',
    'Kırılım sonrası retest beklenen maksimum bar sayısı. Default 30.',
    'number', true, 'normal', ARRAY['validator','classical','retest']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection','validator.classical.retest_tolerance_pct','0.005'::jsonb,'Retest touch tolerance as price fraction.'),
    ('detection','validator.classical.retest_max_bars_after_breakout','30'::jsonb,'Max bars after breakout to wait for retest.')
ON CONFLICT (module, config_key) DO NOTHING;
