-- Validator + target engine config (Faz 13A + 13B).
-- Activates qtss-worker::validator_loop and seeds defaults for
-- qtss-targets resolvers (read at strategy/API time).

-- ── Validator ─────────────────────────────────────────────────────────
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('validator', 'enabled',   '{"enabled": true}'::jsonb,
     'Master on/off for the detection-invalidation worker loop.'),
    ('validator', 'tick_secs', '{"secs": 60}'::jsonb,
     'Loop cadence (seconds). Matches engine tick so detections are rechecked each write pass.'),

    ('validator', 'thresholds.harmonic_break_pct',       '{"value": 0.03}'::jsonb,
     'Harmonic PRZ break tolerance as fraction of XA leg. Close beyond D ± this = invalidate.'),
    ('validator', 'thresholds.range_full_fill_pct',      '{"value": 1.0}'::jsonb,
     'Range zone invalidation — close through the zone beyond this fraction.'),
    ('validator', 'thresholds.gap_close_pct',            '{"value": 0.95}'::jsonb,
     'Gap close threshold — gap considered filled when close crosses this fraction of the original gap.'),
    ('validator', 'thresholds.motive_wave1_buffer_pct',  '{"value": 0.005}'::jsonb,
     'Motive wave-1 break tolerance as fraction of wave-1 level.'),
    ('validator', 'thresholds.smc_break_buffer_pct',     '{"value": 0.003}'::jsonb,
     'SMC event invalidation buffer as fraction of reference price.'),
    ('validator', 'thresholds.orb_reentry_bars',         '{"value": 3}'::jsonb,
     'ORB re-entry fakeout window (bars).'),
    ('validator', 'thresholds.classical_break_pct',      '{"value": 0.002}'::jsonb,
     'Classical/fallback invalidation buffer as fraction of price.')
ON CONFLICT (module, config_key) DO NOTHING;

-- ── Targets ──────────────────────────────────────────────────────────
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('targets', 'harmonic_tp_fibs',         '{"value": [0.382, 0.618, 1.0]}'::jsonb,
     'Fibonacci ratios for TP1/TP2/TP3 measured from D back along the CD leg toward C.'),
    ('targets', 'harmonic_sl_buffer_pct',   '{"value": 0.02}'::jsonb,
     'Harmonic SL buffer beyond D as fraction of XA leg.'),

    ('targets', 'vprofile_max_distance_pct','{"value": 0.05}'::jsonb,
     'Max distance (fraction of entry) a profile level can be before it stops qualifying as a target.'),

    ('targets', 'fib_extensions',           '{"value": [1.272, 1.618, 2.618]}'::jsonb,
     'Extension multipliers for Fib-extension resolver.'),

    ('targets', 'structural_tp_count',      '{"value": 3}'::jsonb,
     'Number of forward-direction pivots to use as TP ladder.'),
    ('targets', 'structural_sl_buffer_pct', '{"value": 0.005}'::jsonb,
     'SL buffer beyond the opposite pivot as fraction of price.'),

    ('targets', 'atr_tp_multipliers',       '{"value": [1.5, 3.0, 5.0]}'::jsonb,
     'TP multipliers in ATR units for the ATR-band fallback resolver.'),
    ('targets', 'atr_sl_multiplier',        '{"value": 1.0}'::jsonb,
     'SL multiplier in ATR units.'),
    ('targets', 'atr_min_abs',              '{"value": 0.000000001}'::jsonb,
     'Minimum viable ATR — below this ATR resolver returns None.')
ON CONFLICT (module, config_key) DO NOTHING;
