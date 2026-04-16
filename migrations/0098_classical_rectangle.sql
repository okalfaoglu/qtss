-- 0098_classical_rectangle.sql
--
-- P5.1 — Rectangle (flat top + flat bottom) detector config keys.
-- Two horizontal bands of equal highs and equal lows over a minimum
-- duration. Direction-neutral: breakout channel decides the side
-- post-breach. Reference: Bulkowski, Edwards & Magee.

SELECT _qtss_register_key(
    'classical.rectangle_max_slope_pct','classical','detection','float',
    '0.002'::jsonb, 'fraction',
    'Rectangle trendline eğim tavanı (|slope|/ref_price, bar başına). Üst ve alt bantlar bu eşiğin altında kalmalı. Default 0.002 = %0.2/bar.',
    'number', true, 'normal', ARRAY['classical','rectangle']);

SELECT _qtss_register_key(
    'classical.rectangle_min_bars','classical','detection','int',
    '15'::jsonb, 'bars',
    'Rectangle minimum süresi (ilk pivot ile son pivot arası bar sayısı). Kısa range gürültüsünü eler. Default 15.',
    'number', true, 'normal', ARRAY['classical','rectangle']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection','classical.rectangle_max_slope_pct','0.002'::jsonb,'Rectangle max trendline slope (fraction per bar).'),
    ('detection','classical.rectangle_min_bars','15'::jsonb,'Rectangle minimum duration in bars.')
ON CONFLICT (module, config_key) DO NOTHING;
