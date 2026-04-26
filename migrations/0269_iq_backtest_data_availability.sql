-- 0269_iq_backtest_data_availability.sql
-- BUG BACKTEST — store the pre-flight data availability matrix
-- alongside each persisted run so the GUI can render which scoring
-- channels actually contributed (vs. silently returned 0 because
-- the backing table was missing or empty in the test window).
--
-- Schema:
--   data_availability JSONB — array of ChannelAvailability rows
--     [{ channel, source, status, rows_in_window, earliest, latest }]
--   When NULL, the run pre-dates this column and the GUI shows
--   "(no probe data — re-run to capture)".

ALTER TABLE iq_backtest_runs
  ADD COLUMN IF NOT EXISTS data_availability JSONB;
