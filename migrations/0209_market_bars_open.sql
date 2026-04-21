-- Live/open bar store — one row per series, continuously overwritten by the
-- worker's WebSocket consumer. Lets the chart endpoints paint the current
-- unclosed bar alongside the closed archive in `market_bars`.
--
-- Why a separate table: `market_bars` is append-only with a UNIQUE constraint
-- on (exchange, segment, symbol, interval, open_time) — once a row is written
-- the bar is considered FINAL. Rewriting it mid-formation would break
-- every downstream detector that assumes immutable closed bars. Hence the
-- split: `market_bars` = archive, `market_bars_open` = single-row running
-- ticker per series.
--
-- Why PK is (exchange, segment, symbol, interval) with NO open_time: only
-- one open bar exists per series at any instant. When a bar closes, the
-- worker writes it to `market_bars` (archive) and the next frame of the
-- NEW open bar simply overwrites this row. Zero growth over time.

CREATE TABLE IF NOT EXISTS market_bars_open (
    exchange     TEXT        NOT NULL,
    segment      TEXT        NOT NULL,
    symbol       TEXT        NOT NULL,
    interval     TEXT        NOT NULL,
    open_time    TIMESTAMPTZ NOT NULL,
    close_time   TIMESTAMPTZ NOT NULL,
    open         NUMERIC     NOT NULL,
    high         NUMERIC     NOT NULL,
    low          NUMERIC     NOT NULL,
    close        NUMERIC     NOT NULL,
    volume       NUMERIC     NOT NULL,
    trade_count  BIGINT      NOT NULL DEFAULT 0,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (exchange, segment, symbol, interval)
);

COMMENT ON TABLE market_bars_open IS 'One row per series — last seen unclosed kline frame. Overwritten by the worker on every WebSocket tick.';
COMMENT ON COLUMN market_bars_open.open_time IS 'open_time of the CURRENT forming bar, not any earlier bar.';
COMMENT ON COLUMN market_bars_open.updated_at IS 'Freshness marker — lets the API decide whether to serve this row or treat it as stale.';
