-- Indicator compute configuration — seed default periods and factors
-- for the 11 new indicators exposed through `/v2/indicators/...`.
-- Every knob is optional: missing rows fall back to Rust defaults in
-- `crates/qtss-api/src/routes/v2_indicators.rs::compute_indicator`.
--
-- Shape: module='indicators', config_key='<name>.<field>',
--        value = {"value": <number>}

INSERT INTO system_config (module, config_key, value, description) VALUES
    -- RSI (Wilder momentum oscillator).
    ('indicators', 'rsi.period',            '{"value": 14}'::jsonb,        'RSI lookback period (Wilder default 14).'),

    -- EMA (dual period overlay on the chart).
    ('indicators', 'ema.fast',              '{"value": 9}'::jsonb,         'Fast EMA period (chart overlay).'),
    ('indicators', 'ema.slow',              '{"value": 21}'::jsonb,        'Slow EMA period (chart overlay).'),

    -- SMA (long-term baseline; 200 = trader consensus).
    ('indicators', 'sma.period',            '{"value": 200}'::jsonb,       'SMA lookback period.'),

    -- Bollinger Bands.
    ('indicators', 'bollinger.period',      '{"value": 20}'::jsonb,        'Bollinger moving-average period.'),
    ('indicators', 'bollinger.stdev',       '{"value": 2.0}'::jsonb,       'Bollinger standard deviation multiplier.'),

    -- MACD.
    ('indicators', 'macd.fast',             '{"value": 12}'::jsonb,        'MACD fast EMA.'),
    ('indicators', 'macd.slow',             '{"value": 26}'::jsonb,        'MACD slow EMA.'),
    ('indicators', 'macd.signal',           '{"value": 9}'::jsonb,         'MACD signal EMA.'),

    -- Stochastic %K/%D.
    ('indicators', 'stochastic.k',          '{"value": 14}'::jsonb,        'Stochastic %K lookback.'),
    ('indicators', 'stochastic.d',          '{"value": 3}'::jsonb,         'Stochastic %D smoothing.'),

    -- ATR.
    ('indicators', 'atr.period',            '{"value": 14}'::jsonb,        'Wilder ATR period.'),

    -- SuperTrend.
    ('indicators', 'supertrend.period',     '{"value": 10}'::jsonb,        'SuperTrend ATR period.'),
    ('indicators', 'supertrend.factor',     '{"value": 3.0}'::jsonb,       'SuperTrend band multiplier (factor × ATR).'),

    -- Ichimoku (Goichi Hosoda original 9/26/52/26 ratio).
    ('indicators', 'ichimoku.tenkan',       '{"value": 9}'::jsonb,         'Tenkan-sen (conversion line) lookback.'),
    ('indicators', 'ichimoku.kijun',        '{"value": 26}'::jsonb,        'Kijun-sen (base line) lookback.'),
    ('indicators', 'ichimoku.senkou_b',     '{"value": 52}'::jsonb,        'Senkou Span B lookback.'),
    ('indicators', 'ichimoku.shift',        '{"value": 26}'::jsonb,        'Forward/backward shift for cloud & lagging span.'),

    -- Donchian.
    ('indicators', 'donchian.period',       '{"value": 20}'::jsonb,        'Donchian channel lookback.'),

    -- Keltner.
    ('indicators', 'keltner.ema_period',    '{"value": 20}'::jsonb,        'Keltner midline EMA period.'),
    ('indicators', 'keltner.atr_period',    '{"value": 10}'::jsonb,        'Keltner ATR period.'),
    ('indicators', 'keltner.mult',          '{"value": 2.0}'::jsonb,       'Keltner band multiplier.'),

    -- Williams %R.
    ('indicators', 'williams_r.period',     '{"value": 14}'::jsonb,        'Williams %R lookback.'),

    -- CMF (Chaikin Money Flow).
    ('indicators', 'cmf.period',            '{"value": 20}'::jsonb,        'Chaikin Money Flow window.'),

    -- Aroon.
    ('indicators', 'aroon.period',          '{"value": 25}'::jsonb,        'Aroon lookback.'),

    -- Parabolic SAR.
    ('indicators', 'psar.acc_start',        '{"value": 0.02}'::jsonb,      'PSAR initial acceleration factor.'),
    ('indicators', 'psar.acc_step',         '{"value": 0.02}'::jsonb,      'PSAR acceleration increment.'),
    ('indicators', 'psar.acc_max',          '{"value": 0.2}'::jsonb,       'PSAR maximum acceleration.'),

    -- Chandelier Exit.
    ('indicators', 'chandelier.period',     '{"value": 22}'::jsonb,        'Chandelier Exit lookback (approx. one trading month).'),
    ('indicators', 'chandelier.mult',       '{"value": 3.0}'::jsonb,       'Chandelier Exit ATR multiplier.'),

    -- TTM Squeeze.
    ('indicators', 'ttm_squeeze.bb_period', '{"value": 20}'::jsonb,        'TTM Squeeze Bollinger period.'),
    ('indicators', 'ttm_squeeze.bb_stdev',  '{"value": 2.0}'::jsonb,       'TTM Squeeze Bollinger stdev.'),
    ('indicators', 'ttm_squeeze.kc_period', '{"value": 20}'::jsonb,        'TTM Squeeze Keltner EMA period.'),
    ('indicators', 'ttm_squeeze.kc_atr_period','{"value": 10}'::jsonb,     'TTM Squeeze Keltner ATR period.'),
    ('indicators', 'ttm_squeeze.kc_mult',   '{"value": 1.5}'::jsonb,       'TTM Squeeze Keltner multiplier (Carter default 1.5).')
ON CONFLICT (module, config_key) DO NOTHING;
