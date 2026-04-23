-- Gap detector config seed — activates the `GapWriter` (qtss-engine)
-- and seeds threshold overrides for the 5 gap specs + island reversal.
-- Every key is optional: missing rows keep `qtss_gap::GapConfig::defaults()`
-- so a blank config still runs the writer.

INSERT INTO system_config (module, config_key, value, description)
VALUES
    ('gap', 'enabled',       '{"enabled": true}'::jsonb,
     'Master on/off for the gap-and-island-reversal engine writer.'),
    ('gap', 'min_score',     '{"score": 0.50}'::jsonb,
     'Minimum structural score (0..1) before a gap match is persisted.'),
    ('gap', 'bars_per_tick', '{"bars": 2000}'::jsonb,
     'Upper bound on recent bars loaded per symbol per tick.'),

    -- Thresholds (see `qtss_gap::GapConfig` rustdoc for meaning).
    ('gap', 'thresholds.min_gap_pct',             '{"value": 0.005}'::jsonb,
     'Minimum gap magnitude as a fraction of close (0.005 = 0.5%).'),
    ('gap', 'thresholds.volume_sma_bars',         '{"value": 20}'::jsonb,
     'SMA window for volume baseline (volume-confirm ratio).'),
    ('gap', 'thresholds.vol_mult_breakaway',      '{"value": 1.5}'::jsonb,
     'Breakaway gap requires volume ≥ this × SMA.'),
    ('gap', 'thresholds.vol_mult_runaway',        '{"value": 1.3}'::jsonb,
     'Runaway gap volume multiplier.'),
    ('gap', 'thresholds.vol_mult_exhaustion',     '{"value": 1.8}'::jsonb,
     'Exhaustion gap volume multiplier.'),
    ('gap', 'thresholds.range_flat_pct',          '{"value": 0.02}'::jsonb,
     'Consolidation range width cap (fraction) for breakaway classification.'),
    ('gap', 'thresholds.consolidation_lookback',  '{"value": 10}'::jsonb,
     'Lookback window (bars) used to measure pre-gap consolidation.'),
    ('gap', 'thresholds.runaway_trend_bars',      '{"value": 5}'::jsonb,
     'Minimum same-sign bar count for runaway trend qualification.'),
    ('gap', 'thresholds.runaway_trend_min_pct',   '{"value": 0.02}'::jsonb,
     'Minimum cumulative return over the trend bar window (fraction).'),
    ('gap', 'thresholds.exhaustion_reversal_bars','{"value": 5}'::jsonb,
     'Reversal must occur within this many bars after the gap for exhaustion.'),
    ('gap', 'thresholds.island_max_bars',         '{"value": 10}'::jsonb,
     'Maximum plateau length between opposing gaps in an island reversal.')
ON CONFLICT (module, config_key) DO NOTHING;
