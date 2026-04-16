-- 0108_wyckoff_per_setup_triggers.sql
--
-- P7.5 — Per-setup entry filter rules.
--
-- (a) Directional trigger-bar gate: the bar that fires the setup must
--     itself close in the setup direction (bullish for Long, bearish
--     for Short) with close-in-range ≥ `trigger_bar_min_close_pos`.
-- (b) New setup: JAC (Jump Across Creek) — Villahermosa §7.4.1.
--     Wide-range bullish breakout bar on above-average volume.

-- ---------------------------------------------------------------------
-- Directional trigger gate (applies to all 8 setup types)
-- ---------------------------------------------------------------------
SELECT _qtss_register_key(
    'wyckoff.setup.require_directional_trigger','setup','detection','bool',
    'true'::jsonb, 'flag',
    'Require the firing bar to close in the setup direction (bullish for Long, bearish for Short).',
    'boolean', true, 'normal', ARRAY['wyckoff','setup','trigger']);

SELECT _qtss_register_key(
    'wyckoff.setup.trigger_bar_min_close_pos','setup','detection','float',
    '0.5'::jsonb, 'ratio',
    'Minimum close-position within the trigger bar range [0..1]. 0.5 = close at or above midpoint for Long.',
    'number', true, 'normal', ARRAY['wyckoff','setup','trigger']);

-- ---------------------------------------------------------------------
-- JAC thresholds
-- ---------------------------------------------------------------------
SELECT _qtss_register_key(
    'wyckoff.setup.jac_min_volume_ratio','setup','detection','float',
    '1.5'::jsonb, 'ratio',
    'JAC breakout bar volume must be ≥ this × 20-bar volume average.',
    'number', true, 'normal', ARRAY['wyckoff','setup','jac']);

SELECT _qtss_register_key(
    'wyckoff.setup.jac_min_range_atr','setup','detection','float',
    '1.2'::jsonb, 'atr',
    'JAC breakout bar range must be ≥ this × ATR.',
    'number', true, 'normal', ARRAY['wyckoff','setup','jac']);

SELECT _qtss_register_key(
    'wyckoff.setup.jac_buffer_atr','setup','detection','float',
    '0.4'::jsonb, 'atr',
    'Tight-SL buffer below the creek for JAC setups, in ATR units.',
    'number', true, 'normal', ARRAY['wyckoff','setup','jac']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection','wyckoff.setup.require_directional_trigger','true'::jsonb,'Gate setups behind a directional trigger bar.'),
    ('detection','wyckoff.setup.trigger_bar_min_close_pos','0.5'::jsonb,'Min close-in-range position for trigger bar.'),
    ('detection','wyckoff.setup.jac_min_volume_ratio','1.5'::jsonb,'JAC bar volume / 20-bar avg min ratio.'),
    ('detection','wyckoff.setup.jac_min_range_atr','1.2'::jsonb,'JAC bar range / ATR min ratio.'),
    ('detection','wyckoff.setup.jac_buffer_atr','0.4'::jsonb,'JAC tight SL buffer in ATR units.')
ON CONFLICT (module, config_key) DO NOTHING;

-- Expand allowed_types default CSV to include jac. If operator already
-- customised it, leave their value alone.
UPDATE system_config
   SET value = '"spring,lps,buec,ut,utad,lpsy,ice_retest,jac"'::jsonb
 WHERE module = 'detection'
   AND config_key = 'wyckoff.setup.allowed_types'
   AND value = '"spring,lps,buec,ut,utad,lpsy,ice_retest"'::jsonb;
