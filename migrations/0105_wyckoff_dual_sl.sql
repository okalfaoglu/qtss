-- 0105_wyckoff_dual_sl.sql
--
-- P7.3 — Dual Stop-Loss (tight + wide / structural) for Wyckoff setups.
-- Villahermosa *Wyckoff 2.0* §7.4.2: every Wyckoff trade has two
-- logically distinct invalidation levels:
--   * Tight  — just past the trigger candle (spring low / UT high)
--              for tight R-based sizing.
--   * Wide   — past the structural range boundary (range_bottom for
--              longs, range_top for shorts). If price closes there,
--              the accumulation / distribution hypothesis is dead.
-- The trade planner picks one via `wyckoff.plan.sl_policy`.

SELECT _qtss_register_key(
    'wyckoff.setup.sl_wide_buffer_atr','setup','detection','float',
    '0.5'::jsonb, 'atr',
    'ATR buffer past the range boundary for the structural (wide) SL. Long → range_bottom − buffer·ATR; short → range_top + buffer·ATR.',
    'number', true, 'normal', ARRAY['wyckoff','setup','sl']);

SELECT _qtss_register_key(
    'wyckoff.plan.sl_policy','setup','detection','enum',
    '"tighter"'::jsonb, 'enum',
    'SL selection policy. Values: tighter (min of tight/adaptive), looser, classical_only (tight), adaptive_only, structural_only (wide), tightest_of_all, widest_of_all.',
    'string', true, 'normal', ARRAY['wyckoff','plan','sl']);

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('detection','wyckoff.setup.sl_wide_buffer_atr','0.5'::jsonb,'ATR buffer for the structural (wide) SL past the range boundary.'),
    ('detection','wyckoff.plan.sl_policy','"tighter"'::jsonb,'SL selection policy (tighter/looser/classical_only/adaptive_only/structural_only/tightest_of_all/widest_of_all).')
ON CONFLICT (module, config_key) DO NOTHING;
