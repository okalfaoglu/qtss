-- indicator_snapshots — one row per (instrument, TF, indicator, bar)
-- capturing the indicator's value(s) at the close of that bar. Enables:
--   * Backtest replay without re-computing indicators on every sim step.
--   * GUI fast-path: /v2/indicators can read cached rows when computing
--     inline isn't necessary (deep zoom, long history).
--   * Feature store for ML: deterministic training windows without
--     re-running Pine ports.
--
-- Multi-output indicators (MACD, Ichimoku, Bollinger, SuperTrend, etc.)
-- collapse their sub-series into a single `values` JSONB column keyed
-- by the sub-name the API already returns (`{"upper": 0.9, "mid": ...}`).
-- Single-output indicators use the same shape with one key — the schema
-- stays uniform so no dispatch on indicator name is needed
-- (CLAUDE.md #1).

CREATE TABLE IF NOT EXISTS indicator_snapshots (
    exchange    TEXT        NOT NULL,
    segment     TEXT        NOT NULL,
    symbol      TEXT        NOT NULL,
    timeframe   TEXT        NOT NULL,
    bar_time    TIMESTAMPTZ NOT NULL,
    indicator   TEXT        NOT NULL,
    values      JSONB       NOT NULL,
    config_hash TEXT,         -- SHA1 of the config JSON that produced
                              -- this row. Lets the reader invalidate
                              -- cached rows when the operator tweaks a
                              -- period/factor via the Config Editor.
    computed_at TIMESTAMPTZ  NOT NULL DEFAULT now(),
    PRIMARY KEY (exchange, segment, symbol, timeframe, bar_time, indicator)
);

-- Hot-path index: "give me the last N snapshots of indicator X for
-- instrument Y" — the GUI pagination pattern. `DESC` ordering by
-- `bar_time` makes reverse-chronological scans index-only.
CREATE INDEX IF NOT EXISTS indicator_snapshots_recent_idx
    ON indicator_snapshots (exchange, segment, symbol, timeframe, indicator, bar_time DESC);

-- Purge index: keep scans of stale rows cheap when we TTL older data.
CREATE INDEX IF NOT EXISTS indicator_snapshots_computed_at_idx
    ON indicator_snapshots (computed_at);

COMMENT ON TABLE indicator_snapshots IS
    'Persisted technical-indicator values per bar. Written by qtss-worker::indicator_persistence_loop on each engine tick; consumed by /v2/indicators fast-path and by backtest replay.';
COMMENT ON COLUMN indicator_snapshots.values IS
    'JSONB map {sub_series → numeric}. Multi-output indicators expose each line as one key; single-output indicators have one key matching the indicator name.';
COMMENT ON COLUMN indicator_snapshots.config_hash IS
    'SHA1 over the indicator config (period, factor, etc.) that produced this row. Cache-validity gate: a config edit changes the hash and invalidates old rows.';

-- Seed config for the persistence loop — enables/disables + list of
-- indicators to materialise per tick. Default list matches the "base"
-- set the chart fetches most often; operators can trim for DB churn or
-- expand for backtest density.
INSERT INTO system_config (module, config_key, value, description) VALUES
    ('indicator_persistence', 'enabled',
     '{"enabled": true}'::jsonb,
     'Master on/off for the indicator snapshot writer. Disable to stop writes entirely (e.g. during migration windows).'),

    ('indicator_persistence', 'tick_secs',
     '{"secs": 60}'::jsonb,
     'Loop cadence in seconds. One engine tick = one new bar-close persistence pass per symbol/TF.'),

    ('indicator_persistence', 'bars_per_tick',
     '{"bars": 300}'::jsonb,
     'Most recent N bars to (re)materialise per tick. Keeping this modest trims churn while still covering rolling re-computes when a prior bar''s indicator value shifts (Wilder smoothing, etc.).'),

    ('indicator_persistence', 'names',
     '{"names": ["rsi","ema","bollinger","macd","atr","supertrend","keltner","ichimoku","donchian","williams_r","cmf","aroon","psar","chandelier"]}'::jsonb,
     'Ordered list of indicator names to persist. The worker loops over this list and calls the same `compute_indicator` dispatch the /v2/indicators endpoint uses.'),

    ('indicator_persistence', 'retention_days',
     '{"days": 90}'::jsonb,
     'TTL for snapshots. A daily purge job deletes rows older than this. 90 = roughly one quarter — enough for feature-window backtests, light enough on DB size.')
ON CONFLICT (module, config_key) DO NOTHING;
