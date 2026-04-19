-- 0172_faz_9b_backfill_progress.sql
--
-- Faz 9B — backfill orchestrator live progress table.
--
-- qtss_ml_training_runs already records a row per orchestrator cycle
-- (trigger_source='backfill'), but that row only captures start and
-- final state — operators can't see intra-cycle progress without
-- tailing worker logs. This table is a live heartbeat keyed by run_id:
-- the orchestrator upserts on every poll tick (~120s default), so the
-- GUI dashboard (Faz 9B 2nd wave) and `systemctl`-less operators can
-- read exactly where the cycle is.
--
-- Resume semantics: the `historical_progressive_scan_state` table
-- (migration 0071) already persists per-symbol cursors, so a crashed
-- orchestrator naturally resumes scanning where it left off. This
-- progress table does NOT drive resume — it just surfaces status.
--
-- Idempotent.

CREATE TABLE IF NOT EXISTS qtss_ml_backfill_progress (
  run_id            UUID PRIMARY KEY REFERENCES qtss_ml_training_runs(id) ON DELETE CASCADE,
  started_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
  last_poll_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  last_growth_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  last_setup_count  BIGINT      NOT NULL DEFAULT 0,
  symbols_active    INTEGER     NOT NULL DEFAULT 0,
  symbols_total     INTEGER     NOT NULL DEFAULT 0,
  bars_scanned      BIGINT      NOT NULL DEFAULT 0,
  detections_inserted BIGINT    NOT NULL DEFAULT 0,
  phase             TEXT        NOT NULL DEFAULT 'running'
                    CHECK (phase IN ('running','plateau_detected','closed')),
  notes             TEXT
);

CREATE INDEX IF NOT EXISTS idx_backfill_progress_last_poll
  ON qtss_ml_backfill_progress(last_poll_at DESC);

COMMENT ON TABLE qtss_ml_backfill_progress IS
  'Faz 9B — live heartbeat for ml_backfill_orchestrator cycles. One row per active or historical backfill run, upserted on each poll tick.';
