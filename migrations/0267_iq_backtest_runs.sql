-- FAZ 26.6 — backtest run persistence.
-- Each completed `IqBacktestRunner.run()` invocation can persist its
-- summary report here so the GUI Backtest Studio + ops dashboards
-- can list / compare runs without re-running.
--
-- The full per-trade detail still lives in the JSONL log file
-- (column `trade_log_path` points at it). This table is the
-- queryable INDEX of runs.

CREATE TABLE IF NOT EXISTS iq_backtest_runs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_tag         TEXT NOT NULL,
    polarity        TEXT NOT NULL CHECK (polarity IN ('dip', 'top')),
    exchange        TEXT NOT NULL,
    segment         TEXT NOT NULL,
    symbol          TEXT NOT NULL,
    timeframe       TEXT NOT NULL,
    start_time      TIMESTAMPTZ NOT NULL,
    end_time        TIMESTAMPTZ NOT NULL,

    -- Config snapshot (full IqBacktestConfig). Stored as JSONB so
    -- a re-run is one call away.
    config          JSONB NOT NULL,

    -- Aggregate metrics from IqBacktestReport.
    bars_processed  BIGINT  NOT NULL DEFAULT 0,
    total_trades    INT     NOT NULL DEFAULT 0,
    wins            INT     NOT NULL DEFAULT 0,
    losses          INT     NOT NULL DEFAULT 0,
    scratches       INT     NOT NULL DEFAULT 0,
    aborted         INT     NOT NULL DEFAULT 0,
    open_at_end     INT     NOT NULL DEFAULT 0,

    win_rate            DOUBLE PRECISION NOT NULL DEFAULT 0,
    avg_win_pct         DOUBLE PRECISION NOT NULL DEFAULT 0,
    avg_loss_pct        DOUBLE PRECISION NOT NULL DEFAULT 0,
    profit_factor       DOUBLE PRECISION NOT NULL DEFAULT 0,
    expectancy_pct      DOUBLE PRECISION NOT NULL DEFAULT 0,
    sharpe_ratio        DOUBLE PRECISION,

    gross_pnl           NUMERIC NOT NULL DEFAULT 0,
    net_pnl             NUMERIC NOT NULL DEFAULT 0,
    starting_equity     NUMERIC NOT NULL DEFAULT 0,
    final_equity        NUMERIC NOT NULL DEFAULT 0,
    peak_equity         NUMERIC NOT NULL DEFAULT 0,
    max_drawdown_pct    DOUBLE PRECISION NOT NULL DEFAULT 0,

    -- Histogram buckets (LossReason name -> count).
    loss_reason_counts  JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- Avg component score on losing trades.
    avg_loss_components JSONB NOT NULL DEFAULT '{}'::jsonb,

    -- Where the per-trade JSONL lives (NULL when --log not passed).
    trade_log_path      TEXT,

    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS iq_backtest_runs_run_tag_idx
    ON iq_backtest_runs (run_tag, created_at DESC);
CREATE INDEX IF NOT EXISTS iq_backtest_runs_universe_idx
    ON iq_backtest_runs (exchange, segment, symbol, timeframe, created_at DESC);
CREATE INDEX IF NOT EXISTS iq_backtest_runs_polarity_idx
    ON iq_backtest_runs (polarity, created_at DESC);
