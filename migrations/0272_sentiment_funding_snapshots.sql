-- 0272_sentiment_funding_snapshots.sql
-- Stub tables for the two scorers that previously errored on
-- "relation does not exist": sentiment_extreme (fear_greed_snapshots)
-- and funding_oi_signals (external_snapshots). The tables are
-- created empty; populating them is a separate ingestion job
-- (alternative.me Fear & Greed API + Binance funding-rate stream).
--
-- The backtest's pre-flight probe transitions from "✗ missing"
-- (the SQL relation literally does not exist) to "✗ empty" (table
-- exists but no rows in window). The scorers themselves return
-- 0.0 for both statuses, but the operator now has a clear
-- distinction between "schema gap" and "data gap" and a target
-- for future ingestion writers.

CREATE TABLE IF NOT EXISTS fear_greed_snapshots (
    captured_at TIMESTAMPTZ PRIMARY KEY,
    value       INTEGER NOT NULL,
    label       TEXT,
    raw_meta    JSONB NOT NULL DEFAULT '{}'::jsonb
);

-- The scorer queries `external_snapshots` for funding-rate / OI
-- per-bar series. Schema mirrors what a future Binance funding
-- writer would populate.
CREATE TABLE IF NOT EXISTS external_snapshots (
    exchange    TEXT NOT NULL,
    segment     TEXT NOT NULL,
    symbol      TEXT NOT NULL,
    timeframe   TEXT NOT NULL,
    bar_time    TIMESTAMPTZ NOT NULL,
    funding_rate NUMERIC,
    open_interest NUMERIC,
    long_short_ratio NUMERIC,
    raw_meta    JSONB NOT NULL DEFAULT '{}'::jsonb,
    PRIMARY KEY (exchange, segment, symbol, timeframe, bar_time)
);

CREATE INDEX IF NOT EXISTS external_snapshots_time_idx
    ON external_snapshots (exchange, segment, symbol, timeframe, bar_time DESC);
