-- 0096_validator_classical_channels.sql
--
-- P2 — Register config keys for the new classical breakout/volume
-- confirmation channels: BreakoutCloseQuality, BreakoutBodyAtr,
-- VolumeConfirmation. Channel thresholds live in system_config per
-- CLAUDE.md #2.

SELECT _qtss_register_key(
    'validator.classical.atr_period','validator','detection','int',
    '14'::jsonb, 'bars',
    'ATR lookback (bars) used by the classical breakout body/ATR channel.',
    'number', true, 'normal', ARRAY['validator','classical']);

SELECT _qtss_register_key(
    'validator.classical.min_body_atr_mult','validator','detection','float',
    '1.0'::jsonb, 'multiplier',
    'Breakout bar body must be >= N * ATR to score in the normal range. Below this the score is dampened.',
    'number', true, 'normal', ARRAY['validator','classical']);

SELECT _qtss_register_key(
    'validator.classical.max_body_atr_mult','validator','detection','float',
    '3.0'::jsonb, 'multiplier',
    'Upper cap for breakout body/ATR before the bar is considered climactic; score saturates at 1.0.',
    'number', true, 'normal', ARRAY['validator','classical']);

SELECT _qtss_register_key(
    'validator.classical.min_breakout_vol_mult','validator','detection','float',
    '1.5'::jsonb, 'multiplier',
    'Breakout volume must be >= N * pattern-window average to score full expansion weight.',
    'number', true, 'normal', ARRAY['validator','classical']);

SELECT _qtss_register_key(
    'validator.classical.max_late_to_early_vol_ratio','validator','detection','float',
    '1.0'::jsonb, 'ratio',
    'Late-window avg volume over early-window avg inside the pattern. Ratios >= this cap score 0 (no contraction).',
    'number', true, 'normal', ARRAY['validator','classical']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection','validator.classical.atr_period','14'::jsonb,'ATR period for classical breakout channel.'),
    ('detection','validator.classical.min_body_atr_mult','1.0'::jsonb,'Min breakout body/ATR multiple.'),
    ('detection','validator.classical.max_body_atr_mult','3.0'::jsonb,'Max breakout body/ATR multiple.'),
    ('detection','validator.classical.min_breakout_vol_mult','1.5'::jsonb,'Min breakout volume multiple.'),
    ('detection','validator.classical.max_late_to_early_vol_ratio','1.0'::jsonb,'Max late/early pattern volume ratio.')
ON CONFLICT (module, config_key) DO NOTHING;
