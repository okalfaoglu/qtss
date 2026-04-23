-- 0218_drop_legacy_v2_detections_stack.sql
--
-- Full demolition of the pre-Pine-port v2 detection pipeline.
--
-- Context:
--   The old orchestrator / validator / sweeper / tbm / confluence stack
--   wrote to `qtss_v2_detections` and a constellation of satellite
--   tables (setups, outcomes, ml_predictions, llm_verdicts, etc.).
--   Elliott + harmonic writers now populate the new `detections` table
--   directly; the entire old stack is being torn out in favour of a
--   single `qtss-engine` crate that will orchestrate pattern writers
--   against the new schema.
--
-- Scope: drop every table/view that only the legacy stack reads or
--   writes. Downstream (`detections`, `pivots`, `market_bars`,
--   `engine_symbols`, etc.) is untouched.
--
-- Why CASCADE everywhere: some of these tables have FKs to each other
--   (setups → detections, outcomes → detections, etc.). CASCADE lets
--   us run the drops in any order without hunting for the topological
--   sort. The MV must be dropped before the table it reads.

BEGIN;

DROP MATERIALIZED VIEW IF EXISTS qtss_v2_detection_outcome_stats CASCADE;

-- Core detection + lifecycle tables
DROP TABLE IF EXISTS qtss_v2_detection_outcomes CASCADE;
DROP TABLE IF EXISTS qtss_v2_detections         CASCADE;

-- Setup engine (old) — NOT dropping qtss_v2_setups / qtss_v2_setup_events /
-- qtss_v2_setup_rejections / qtss_setup_lifecycle_events / qtss_setup_outcomes.
-- The Faz 9.7.x subscription pipeline (setup_watcher + setup_publisher +
-- selector + x_publisher + digest) still reads/writes those tables. The
-- old `v2_setup_loop` writer was deleted; the new qtss-engine will take
-- over as producer. Tables themselves stay.

-- Confluence + correlation
DROP TABLE IF EXISTS qtss_v2_confluence          CASCADE;
DROP TABLE IF EXISTS qtss_v2_correlation_groups  CASCADE;
DROP TABLE IF EXISTS market_confluence_snapshots CASCADE;

-- ML pipeline (predictions, drift, training runs, breaker, backfill)
DROP TABLE IF EXISTS qtss_ml_predictions      CASCADE;
DROP TABLE IF EXISTS qtss_ml_drift_snapshots  CASCADE;
DROP TABLE IF EXISTS qtss_ml_training_runs    CASCADE;
DROP TABLE IF EXISTS qtss_ml_breaker_events   CASCADE;
DROP TABLE IF EXISTS qtss_ml_backfill_progress CASCADE;
DROP TABLE IF EXISTS qtss_features_snapshot   CASCADE;

-- LLM judge
DROP TABLE IF EXISTS qtss_llm_verdicts CASCADE;

-- Risk / drawdown snapshots
DROP TABLE IF EXISTS account_drawdown_snapshots CASCADE;

-- Legacy onchain bridge (old stack only — not the Nansen feed itself)
DROP TABLE IF EXISTS qtss_v2_onchain_metrics CASCADE;

-- Legacy Wyckoff detector output (Faz 10 will rewrite against new schema)
DROP TABLE IF EXISTS wyckoff_structures CASCADE;

COMMIT;
