-- 0270_iq_backtest_equity_curve.sql
-- Per-trade equity curve as JSONB on iq_backtest_runs. The GUI
-- renders this as a line chart in the run detail; computation lives
-- in IqBacktestReport::aggregate so backtest CLI runs and GUI
-- dispatch runs both populate it the same way.
--
-- Shape:
--   data_availability JSONB → existing (0269)
--   equity_curve JSONB → array of EquityPoint
--     [{ trade_index, time, net_pnl_cum, equity, peak_equity, drawdown_pct }]
--
-- NULL when the run pre-dates this column.

ALTER TABLE iq_backtest_runs
  ADD COLUMN IF NOT EXISTS equity_curve JSONB;
