-- Range detector config seed — activates the `RangeWriter` (qtss-engine)
-- and seeds sub-detector toggles + per-zone thresholds. Four sub-
-- detectors: FVG, Order Block, Liquidity Pool, Equal Levels. Each one
-- can be flipped off independently via `range.<sub>.enabled`.
--
-- Shape (one row per key):
--   module        = 'range'
--   config_key    = 'enabled' | 'min_score' | 'bars_per_tick' | 'atr_period'
--                 | '<sub>.enabled'
--                 | '<sub>.<field>'             -- numeric / bool knob
--   value (jsonb) = {"enabled": bool}            -- for flags
--                 | {"score":  0.50}             -- min_score
--                 | {"bars":   2000}             -- bars_per_tick
--                 | {"period": 14}               -- atr_period
--                 | {"value":  <number|bool>}    -- per-sub field
--
-- Dispatch in `crates/qtss-engine/src/writers/range.rs` maps
-- `<sub>.<field>` → Rust struct field via individual `load_sub_num` /
-- `load_sub_bool` calls. Unknown keys are ignored so forward-compat
-- config rows do not break the writer (CLAUDE.md #1).

INSERT INTO system_config (module, config_key, value, description)
VALUES
    -- Master toggles.
    ('range', 'enabled',        '{"enabled": true}'::jsonb,
     'Master on/off for the range-zone engine writer inside qtss-engine. Gates the RangeWriter; individual sub-detectors have their own enable flags below.'),
    ('range', 'min_score',      '{"score": 0.50}'::jsonb,
     'Global quality floor applied after every sub-detector. Zones below this score are dropped before persistence. 0.50 keeps mid/high-quality zones.'),
    ('range', 'bars_per_tick',  '{"bars": 2000}'::jsonb,
     'Upper bound on recent bars loaded per symbol per tick. All four range detectors share one ATR pass over this bar slice.'),
    ('range', 'atr_period',     '{"period": 14}'::jsonb,
     'Wilder ATR period used for zone-size / impulse / cluster thresholds. 14 matches standard technical-analysis convention.'),

    -- FVG (Fair Value Gap) sub-detector.
    ('range', 'fvg.enabled',             '{"enabled": true}'::jsonb,
     'Enable the 3-candle Fair Value Gap detector (bullish: c1.high < c3.low; bearish: c1.low > c3.high).'),
    ('range', 'fvg.min_gap_atr_frac',    '{"value": 0.5}'::jsonb,
     'Minimum gap size as a fraction of ATR. 0.5 filters micro-gaps below half an ATR.'),
    ('range', 'fvg.scan_lookback',       '{"value": 50}'::jsonb,
     'How many recent bars to scan for FVG candidates per tick.'),
    ('range', 'fvg.unfilled_only',       '{"value": true}'::jsonb,
     'When true, drop any FVG that price has already filled — only un-mitigated gaps are persisted.'),
    ('range', 'fvg.volume_spike_mult',   '{"value": 1.2}'::jsonb,
     'Candle-2 volume must exceed this × SMA(volume) to set the `volume_confirmed` flag.'),

    -- Order Block sub-detector.
    ('range', 'order_block.enabled',            '{"enabled": true}'::jsonb,
     'Enable the Order Block detector: last opposing candle before a ≥ impulse_atr_mult × ATR impulse.'),
    ('range', 'order_block.impulse_atr_mult',   '{"value": 2.0}'::jsonb,
     'Impulse size threshold as an ATR multiple. Lower = more OBs, higher = rarer/stronger.'),
    ('range', 'order_block.impulse_candles',    '{"value": 3}'::jsonb,
     'Number of candles the impulse must span after the OB candle.'),
    ('range', 'order_block.scan_lookback',      '{"value": 50}'::jsonb,
     'How many recent bars to scan for OB candidates per tick.'),
    ('range', 'order_block.unmitigated_only',   '{"value": true}'::jsonb,
     'When true, drop OBs price has already returned to and through (mitigated).'),
    ('range', 'order_block.volume_spike_mult',  '{"value": 1.3}'::jsonb,
     'Impulse volume must exceed this × SMA(volume) to set `volume_confirmed`.'),

    -- Liquidity Pool sub-detector.
    ('range', 'liquidity_pool.enabled',                  '{"enabled": true}'::jsonb,
     'Enable the Liquidity Pool detector: fractal-pivot clustering within ATR × cluster_atr_mult.'),
    ('range', 'liquidity_pool.pivot_window',             '{"value": 3}'::jsonb,
     'Fractal half-window for pivot detection (pivot = bar extreme over ±N bars).'),
    ('range', 'liquidity_pool.cluster_atr_mult',         '{"value": 0.3}'::jsonb,
     'Cluster tolerance as ATR multiple; pivots within this distance are merged into one pool.'),
    ('range', 'liquidity_pool.min_touches',              '{"value": 2}'::jsonb,
     'Minimum pivot touches required to qualify as a pool.'),
    ('range', 'liquidity_pool.sweep_max_penetration_atr','{"value": 0.5}'::jsonb,
     'Max close-through penetration (in ATR) that still counts as a sweep rather than a clean break.'),
    ('range', 'liquidity_pool.scan_lookback',            '{"value": 100}'::jsonb,
     'How many recent bars to scan for pool-forming pivots per tick.'),

    -- Equal Levels sub-detector.
    ('range', 'equal_levels.enabled',             '{"enabled": true}'::jsonb,
     'Enable the Equal Highs / Equal Lows detector (ATR × equal_tolerance_atr clustering).'),
    ('range', 'equal_levels.pivot_window',        '{"value": 3}'::jsonb,
     'Fractal half-window for pivot detection.'),
    ('range', 'equal_levels.equal_tolerance_atr', '{"value": 0.15}'::jsonb,
     'Equality tolerance as ATR multiple; swings within this are considered "equal".'),
    ('range', 'equal_levels.min_bar_distance',    '{"value": 5}'::jsonb,
     'Minimum bar distance between equal pivots (filters back-to-back duplicates).'),
    ('range', 'equal_levels.scan_lookback',       '{"value": 100}'::jsonb,
     'How many recent bars to scan for equal-level candidates per tick.')
ON CONFLICT (module, config_key) DO NOTHING;
