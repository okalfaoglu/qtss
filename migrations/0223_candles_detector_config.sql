-- Candle-pattern detector config seed — activates `CandlesWriter`
-- (qtss-engine) and seeds optional threshold overrides. Defaults from
-- `qtss_candles::CandleConfig::default()` already produce a sensible
-- operating point for 15m+ timeframes.

INSERT INTO system_config (module, config_key, value, description)
VALUES
    ('candle', 'enabled',       '{"enabled": true}'::jsonb,
     'Master on/off for the candlestick-pattern engine writer.'),
    ('candle', 'min_score',     '{"score": 0.60}'::jsonb,
     'Minimum structural score (0..1) before a candle match is persisted. Candle patterns are low-reliability in isolation — stricter default than the formation detectors.'),
    ('candle', 'bars_per_tick', '{"bars": 2000}'::jsonb,
     'Upper bound on recent bars loaded per symbol per tick.'),

    ('candle', 'thresholds.doji_body_ratio_max',           '{"value": 0.10}'::jsonb,
     'Doji body / range ratio cap. Below this the candle qualifies as a doji.'),
    ('candle', 'thresholds.marubozu_shadow_ratio_max',     '{"value": 0.05}'::jsonb,
     'Marubozu shadow / range ratio cap (nearly no wicks).'),
    ('candle', 'thresholds.hammer_lower_shadow_ratio_min', '{"value": 2.0}'::jsonb,
     'Hammer lower-shadow / body ratio floor.'),
    ('candle', 'thresholds.tweezer_price_tol',             '{"value": 0.002}'::jsonb,
     'Tweezer top/bottom price-equality tolerance (fraction of mid).'),
    ('candle', 'thresholds.trend_context_bars',            '{"value": 5}'::jsonb,
     'Bars to look back for trend-context classification.'),
    ('candle', 'thresholds.trend_context_min_pct',         '{"value": 0.015}'::jsonb,
     'Minimum |return| over the context window to call a prior uptrend / downtrend.'),
    ('candle', 'thresholds.min_timeframe_seconds',         '{"value": 900}'::jsonb,
     'Timeframe gate in seconds. 900 = 15m; sub-threshold TFs are skipped because candle patterns emit near-random signal there.')
ON CONFLICT (module, config_key) DO NOTHING;
