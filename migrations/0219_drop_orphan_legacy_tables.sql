-- 0219 — Drop orphan legacy tables with zero Rust references.
--
-- Scan of migrations/*.sql + crates/**/*.rs (word-boundary match,
-- migrations/ excluded) found three tables that are created but
-- never read/written anywhere in the codebase:
--
--   * backtest_runs                     — 0001_qtss_baseline.sql
--   * historical_progressive_scan_state — 0071_historical_progressive_scan.sql
--   * report_exchange_class             — 0200_report_performance.sql
--
-- None of the kept pipelines (Faz 9.7.x setup subscription,
-- pivots_writer_loop, detections_writer_loop, elliott/harmonic
-- detectors, reconcile, regime) touch these tables. Safe to drop.

DROP TABLE IF EXISTS backtest_runs CASCADE;
DROP TABLE IF EXISTS historical_progressive_scan_state CASCADE;
DROP TABLE IF EXISTS report_exchange_class CASCADE;
