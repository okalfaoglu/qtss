-- Confluence engine — persistent aggregation of `detections` across
-- all pattern families into a single per-symbol-per-TF score.
-- Faz 12D.

CREATE TABLE IF NOT EXISTS confluence_snapshots (
    exchange       TEXT        NOT NULL,
    segment        TEXT        NOT NULL,
    symbol         TEXT        NOT NULL,
    timeframe      TEXT        NOT NULL,
    computed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    bull_score     DOUBLE PRECISION NOT NULL,
    bear_score     DOUBLE PRECISION NOT NULL,
    net_score      DOUBLE PRECISION NOT NULL,
    confidence     DOUBLE PRECISION NOT NULL,
    verdict        TEXT        NOT NULL,
    contributors   JSONB       NOT NULL,
    regime         TEXT,
    PRIMARY KEY (exchange, segment, symbol, timeframe, computed_at)
);

CREATE INDEX IF NOT EXISTS confluence_snapshots_recent_idx
    ON confluence_snapshots (exchange, segment, symbol, timeframe, computed_at DESC);

COMMENT ON TABLE confluence_snapshots IS
    'Per-tick aggregated bull/bear score derived from recent detections across all pattern families. Written by qtss-worker::confluence_loop, read by /v2/confluence API and the chart widget.';

-- Config seed — weights + thresholds. All optional; missing rows fall
-- back to `ConfluenceConfig::default()` in qtss-analysis.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('confluence', 'enabled',          '{"enabled": true}'::jsonb,
     'Master on/off for the confluence aggregation loop.'),
    ('confluence', 'tick_secs',        '{"secs": 60}'::jsonb,
     'Loop cadence — one aggregation pass per symbol/TF every N seconds.'),
    ('confluence', 'window_minutes',   '{"value": 60}'::jsonb,
     'Lookback window (minutes) for detections considered fresh.'),
    ('confluence', 'strong_threshold', '{"value": 1.5}'::jsonb,
     '|net_score| threshold above which verdict becomes StrongBull / StrongBear.'),

    -- Per-family weights (defaults favour structural patterns over raw candles).
    ('confluence', 'weights.motive',      '{"value": 1.2}'::jsonb, 'Elliott impulse weight.'),
    ('confluence', 'weights.abc',         '{"value": 1.0}'::jsonb, 'Elliott ABC weight.'),
    ('confluence', 'weights.harmonic',    '{"value": 1.2}'::jsonb, 'Harmonic pattern weight.'),
    ('confluence', 'weights.classical',   '{"value": 1.0}'::jsonb, 'Classical chart pattern weight.'),
    ('confluence', 'weights.range',       '{"value": 0.9}'::jsonb, 'Range/FVG/OB weight.'),
    ('confluence', 'weights.gap',         '{"value": 0.8}'::jsonb, 'Gap pattern weight.'),
    ('confluence', 'weights.candle',      '{"value": 0.3}'::jsonb, 'Candlestick pattern weight (low — noise-prone alone).'),
    ('confluence', 'weights.orb',         '{"value": 0.8}'::jsonb, 'Opening Range Breakout weight.'),
    ('confluence', 'weights.smc',         '{"value": 1.1}'::jsonb, 'Smart Money Concepts event weight.'),
    ('confluence', 'weights.derivatives', '{"value": 0.9}'::jsonb, 'Derivatives-flow event weight.'),
    ('confluence', 'weights.orderflow',   '{"value": 0.9}'::jsonb, 'Order-flow event weight.'),

    -- Regime adjusters (per family × regime). Empty by default — the
    -- strategist can tune as live hit-rate data accumulates.
    ('confluence', 'regime.harmonic:trending_up',   '{"value": 0.6}'::jsonb,
     'Harmonic signals weighted down in strong uptrend regimes (mean-reversion edge erodes).'),
    ('confluence', 'regime.harmonic:trending_down', '{"value": 0.6}'::jsonb,
     'Same for downtrends.'),
    ('confluence', 'regime.motive:ranging',         '{"value": 0.5}'::jsonb,
     'Elliott impulse assumptions break in ranging regimes.'),
    ('confluence', 'regime.smc:trending_up',        '{"value": 1.2}'::jsonb,
     'SMC breaks are higher-quality in established trends.'),
    ('confluence', 'regime.smc:trending_down',      '{"value": 1.2}'::jsonb,
     'Same for downtrends.')
ON CONFLICT (module, config_key) DO NOTHING;
