-- Smart Money Concepts detector config (Faz 12A).
--
-- Enables the `SmcWriter` in qtss-engine and seeds thresholds for the
-- five SMC event families: BOS / CHoCH / MSS / LiquiditySweep / FVI.
-- Defaults match `qtss_smc::SmcConfig::default()` — rows are optional,
-- missing keys fall back to the Rust default.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('smc', 'enabled',         '{"enabled": true}'::jsonb,
     'Master on/off for the Smart Money Concepts engine writer.'),
    ('smc', 'min_score',       '{"score": 0.55}'::jsonb,
     'Minimum event score (0..1) before a SMC event is persisted.'),
    ('smc', 'bars_per_tick',   '{"bars": 2000}'::jsonb,
     'Upper bound on recent bars loaded per symbol per tick.'),
    ('smc', 'pivots_per_slot', '{"count": 500}'::jsonb,
     'Upper bound on recent pivots loaded per slot per tick.'),

    -- BOS / CHoCH / MSS.
    ('smc', 'thresholds.break_confirm_bars', '{"value": 1}'::jsonb,
     'How many consecutive closes confirm a structural break (1 = standard Pine port).'),
    ('smc', 'thresholds.mss_close_cushion_pct', '{"value": 0.002}'::jsonb,
     'Extra cushion MSS requires beyond the raw swing close, as fraction of price. 0.002 = 20 bps filter against fakeouts.'),

    -- Liquidity sweep.
    ('smc', 'thresholds.sweep_wick_penetration_pct', '{"value": 0.001}'::jsonb,
     'Minimum wick penetration past a prior swing for a sweep to count (fraction of price).'),
    ('smc', 'thresholds.sweep_reject_frac', '{"value": 0.5}'::jsonb,
     'Fraction of the sweep excursion the close must recover within sweep_reject_bars.'),
    ('smc', 'thresholds.sweep_reject_bars', '{"value": 2}'::jsonb,
     'Bars (including the sweep bar) within which the rejection must complete.'),

    -- Fair Value Imbalance.
    ('smc', 'thresholds.fvi_min_gap_atr_frac', '{"value": 0.8}'::jsonb,
     'Minimum FVI gap size as a fraction of short-window ATR.'),
    ('smc', 'thresholds.fvi_volume_spike_mult', '{"value": 1.5}'::jsonb,
     'Middle-candle volume must be ≥ this × SMA(volume, 20) for FVI to qualify.'),

    -- Global scan window.
    ('smc', 'thresholds.scan_lookback', '{"value": 100}'::jsonb,
     'Recent bars scanned per symbol per tick for every SMC event family.')
ON CONFLICT (module, config_key) DO NOTHING;
