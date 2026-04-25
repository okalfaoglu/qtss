-- FAZ 25.1.1 — market_bars completeness loop.
--
-- Problem the user surfaced: BTCUSDT 1d futures had 2409 bars in
-- backfill_progress but only 100 in market_bars (the rest were
-- somewhere lost — likely an old retention pass that wasn't followed
-- by a fresh backfill). The chart only showed those 100 days, so
-- zooming out on 1d was useless.
--
-- This migration seeds a periodic gap detector + auto-backfill
-- worker loop that:
--   1. Walks `engine_symbols` where `state = 'live'`.
--   2. For each (exchange, segment, symbol, interval) counts
--      market_bars rows, computes the listing-to-now expected count
--      and inspects the time series for internal gaps.
--   3. When the actual count is lower than expected, calls
--      backfill_binance_public_klines to fill from the oldest known
--      bar back to listing date, OR forward-fills internal gaps.
--   4. Records what it filled in `market_bars_gap_events` so the
--      downstream listeners (engine writers, indicator persistence,
--      confluence loop) can re-run on the affected slice.
--
-- The PG NOTIFY channel `qtss_market_bars_gap_filled` carries the
-- (exchange, segment, symbol, interval, oldest_filled, newest_filled)
-- payload as JSON; engine writers listen and refresh their slice.

INSERT INTO system_config (module, config_key, value, description) VALUES
    ('market_bars_gap_loop', 'enabled',
     '{"enabled": true}'::jsonb,
     'Master switch for the periodic gap detector. When false the loop sleeps an hour and skips checks.'),
    ('market_bars_gap_loop', 'tick_secs',
     '{"secs": 1800}'::jsonb,
     'Cadence in seconds (default 30 min). Each tick walks every live engine_symbol and triggers a backfill where the actual market_bars count is short of expected.'),
    ('market_bars_gap_loop', 'min_completeness_pct',
     '{"value": 0.95}'::jsonb,
     'When (actual / expected) drops below this fraction, fire a backfill. 0.95 leaves room for the most-recent unfinished bar without thrashing.'),
    ('market_bars_gap_loop', 'max_backfill_per_tick',
     '{"value": 5}'::jsonb,
     'Max number of (symbol, interval) pairs to backfill per tick. Caps API rate burn — Binance allows ~1200 weight/min and each kline page is 1 weight.'),
    ('market_bars_gap_loop', 'pages_per_run',
     '{"value": 50}'::jsonb,
     'Max kline-paginate pages per backfill invocation (each page = 1000 bars). 50 pages = 50k bars max per pair per tick.')
ON CONFLICT (module, config_key) DO NOTHING;

-- Audit ledger for every gap-fill event. Listeners can also read this
-- if they prefer polling over pg_notify.
CREATE TABLE IF NOT EXISTS market_bars_gap_events (
    id              bigserial PRIMARY KEY,
    fired_at        timestamptz NOT NULL DEFAULT now(),
    exchange        text NOT NULL,
    segment         text NOT NULL,
    symbol          text NOT NULL,
    interval        text NOT NULL,
    expected_bars   bigint,
    actual_bars     bigint NOT NULL,
    bars_upserted   bigint NOT NULL DEFAULT 0,
    pages_fetched   integer NOT NULL DEFAULT 0,
    oldest_filled   timestamptz,
    newest_filled   timestamptz,
    success         boolean NOT NULL DEFAULT true,
    error_text      text
);

CREATE INDEX IF NOT EXISTS market_bars_gap_events_recent_idx
    ON market_bars_gap_events (exchange, segment, symbol, interval, fired_at DESC);
