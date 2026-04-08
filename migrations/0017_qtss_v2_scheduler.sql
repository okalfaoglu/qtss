-- 0017_qtss_v2_scheduler.sql
--
-- Faz 0.6 — Scheduler tables.
--
-- Two tables:
--   * scheduled_jobs : the catalog. One row per recurring task (e.g.
--                      "pull Nansen smart-money flows every 5 minutes").
--   * job_runs       : execution history. One row per attempt, success
--                      or failure, with timing + error payload.
--
-- The scheduler crate (qtss-scheduler) leases due jobs by atomically
-- bumping `next_run_at` and writing a `running` row into job_runs.
-- This makes horizontal scaling safe: any number of scheduler workers
-- can poll the same table without duplicating work.

BEGIN;

-- ---------------------------------------------------------------------------
-- scheduled_jobs
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS scheduled_jobs (
    id              BIGSERIAL PRIMARY KEY,
    name            TEXT        NOT NULL UNIQUE,        -- e.g. "nansen.smart_money_pull"
    description     TEXT        NULL,
    -- "interval:30s" | "cron:0 */5 * * * *". Parsed by qtss-scheduler.
    schedule_kind   TEXT        NOT NULL,
    schedule_expr   TEXT        NOT NULL,
    handler         TEXT        NOT NULL,                -- logical handler key (registered in code)
    payload         JSONB       NOT NULL DEFAULT '{}'::jsonb,
    enabled         BOOLEAN     NOT NULL DEFAULT TRUE,
    timeout_s       INT         NOT NULL DEFAULT 60,
    max_retries     INT         NOT NULL DEFAULT 3,
    next_run_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_run_at     TIMESTAMPTZ NULL,
    last_status     TEXT        NULL,                    -- success | failed | timeout
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT scheduled_jobs_kind_chk
        CHECK (schedule_kind IN ('interval','cron')),
    CONSTRAINT scheduled_jobs_status_chk
        CHECK (last_status IS NULL OR last_status IN ('success','failed','timeout'))
);

CREATE INDEX IF NOT EXISTS scheduled_jobs_due_idx
    ON scheduled_jobs (next_run_at) WHERE enabled = TRUE;

-- ---------------------------------------------------------------------------
-- job_runs
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS job_runs (
    id          BIGSERIAL PRIMARY KEY,
    job_id      BIGINT      NOT NULL REFERENCES scheduled_jobs(id) ON DELETE CASCADE,
    started_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMPTZ NULL,
    status      TEXT        NOT NULL DEFAULT 'running',  -- running | success | failed | timeout
    attempt     INT         NOT NULL DEFAULT 1,
    error       TEXT        NULL,
    output      JSONB       NULL,                         -- handler-defined result blob
    worker_id   TEXT        NULL,                         -- which scheduler instance ran it
    CONSTRAINT job_runs_status_chk
        CHECK (status IN ('running','success','failed','timeout'))
);

CREATE INDEX IF NOT EXISTS job_runs_job_idx       ON job_runs (job_id, started_at DESC);
CREATE INDEX IF NOT EXISTS job_runs_status_idx    ON job_runs (status) WHERE status = 'running';

COMMIT;
