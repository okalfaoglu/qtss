//! Job catalog persistence + due-job leasing.
//!
//! `lease_due` is the heart of horizontal scaling: it picks one due job
//! atomically (FOR UPDATE SKIP LOCKED in PG) and bumps `next_run_at` so
//! a sibling worker on the same row sees no work to do. The memory store
//! mirrors this contract for unit tests.

use crate::error::SchedulerResult;
use crate::schedule::{next_after, Schedule};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct NewJob {
    pub name: String,
    pub description: Option<String>,
    pub schedule_kind: String,
    pub schedule_expr: String,
    pub handler: String,
    pub payload: serde_json::Value,
    pub timeout_s: i32,
    pub max_retries: i32,
}

#[derive(Debug, Clone)]
pub struct JobRecord {
    pub id: i64,
    pub name: String,
    pub schedule_kind: String,
    pub schedule_expr: String,
    pub handler: String,
    pub payload: serde_json::Value,
    pub enabled: bool,
    pub timeout_s: i32,
    pub max_retries: i32,
    pub next_run_at: DateTime<Utc>,
}

/// Outcome reported back after a handler runs. The store turns this into
/// a row update on `scheduled_jobs` and a finalization on `job_runs`.
#[derive(Debug, Clone)]
pub enum RunOutcome {
    Success(serde_json::Value),
    Failed(String),
    Timeout,
}

#[async_trait]
pub trait JobStore: Send + Sync {
    async fn upsert(&self, job: NewJob) -> SchedulerResult<JobRecord>;

    /// Atomically claim a single due job, advancing its `next_run_at` to
    /// the next scheduled fire time and inserting a `running` job_run row.
    /// Returns `Ok(None)` if no jobs are due.
    async fn lease_due(
        &self,
        now: DateTime<Utc>,
        worker_id: &str,
    ) -> SchedulerResult<Option<(JobRecord, i64)>>;

    /// Finalize a previously leased run with its outcome.
    async fn finish_run(&self, run_id: i64, outcome: RunOutcome) -> SchedulerResult<()>;
}

// ---------------------------------------------------------------------------
// PgJobStore
// ---------------------------------------------------------------------------

pub struct PgJobStore {
    pool: PgPool,
}

impl PgJobStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl JobStore for PgJobStore {
    async fn upsert(&self, job: NewJob) -> SchedulerResult<JobRecord> {
        let row: (
            i64,
            String,
            String,
            String,
            String,
            serde_json::Value,
            bool,
            i32,
            i32,
            DateTime<Utc>,
        ) = sqlx::query_as(
            "INSERT INTO scheduled_jobs
                (name, description, schedule_kind, schedule_expr, handler, payload, timeout_s, max_retries)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (name) DO UPDATE SET
                schedule_kind = EXCLUDED.schedule_kind,
                schedule_expr = EXCLUDED.schedule_expr,
                handler       = EXCLUDED.handler,
                payload       = EXCLUDED.payload,
                timeout_s     = EXCLUDED.timeout_s,
                max_retries   = EXCLUDED.max_retries,
                updated_at    = NOW()
             RETURNING id, name, schedule_kind, schedule_expr, handler, payload,
                       enabled, timeout_s, max_retries, next_run_at",
        )
        .bind(&job.name)
        .bind(&job.description)
        .bind(&job.schedule_kind)
        .bind(&job.schedule_expr)
        .bind(&job.handler)
        .bind(&job.payload)
        .bind(job.timeout_s)
        .bind(job.max_retries)
        .fetch_one(&self.pool)
        .await?;
        Ok(JobRecord {
            id: row.0,
            name: row.1,
            schedule_kind: row.2,
            schedule_expr: row.3,
            handler: row.4,
            payload: row.5,
            enabled: row.6,
            timeout_s: row.7,
            max_retries: row.8,
            next_run_at: row.9,
        })
    }

    async fn lease_due(
        &self,
        now: DateTime<Utc>,
        worker_id: &str,
    ) -> SchedulerResult<Option<(JobRecord, i64)>> {
        let mut tx = self.pool.begin().await?;
        // SKIP LOCKED is what makes this safe with N parallel schedulers.
        let row: Option<(
            i64,
            String,
            String,
            String,
            String,
            serde_json::Value,
            bool,
            i32,
            i32,
            DateTime<Utc>,
        )> = sqlx::query_as(
            "SELECT id, name, schedule_kind, schedule_expr, handler, payload,
                    enabled, timeout_s, max_retries, next_run_at
             FROM scheduled_jobs
             WHERE enabled = TRUE AND next_run_at <= $1
             ORDER BY next_run_at ASC
             LIMIT 1
             FOR UPDATE SKIP LOCKED",
        )
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(r) = row else {
            tx.commit().await?;
            return Ok(None);
        };

        let job = JobRecord {
            id: r.0,
            name: r.1,
            schedule_kind: r.2,
            schedule_expr: r.3,
            handler: r.4,
            payload: r.5,
            enabled: r.6,
            timeout_s: r.7,
            max_retries: r.8,
            next_run_at: r.9,
        };

        let schedule = Schedule::parse(&job.schedule_kind, &job.schedule_expr)?;
        let next = next_after(&schedule, now)?;

        sqlx::query(
            "UPDATE scheduled_jobs SET next_run_at = $1, last_run_at = $2, updated_at = NOW()
             WHERE id = $3",
        )
        .bind(next)
        .bind(now)
        .bind(job.id)
        .execute(&mut *tx)
        .await?;

        let run_id: (i64,) = sqlx::query_as(
            "INSERT INTO job_runs (job_id, started_at, status, worker_id)
             VALUES ($1, $2, 'running', $3) RETURNING id",
        )
        .bind(job.id)
        .bind(now)
        .bind(worker_id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(Some((job, run_id.0)))
    }

    async fn finish_run(&self, run_id: i64, outcome: RunOutcome) -> SchedulerResult<()> {
        let (status, error, output): (&str, Option<String>, Option<serde_json::Value>) =
            match outcome {
                RunOutcome::Success(out) => ("success", None, Some(out)),
                RunOutcome::Failed(msg) => ("failed", Some(msg), None),
                RunOutcome::Timeout => ("timeout", Some("handler timed out".into()), None),
            };

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE job_runs SET finished_at = NOW(), status = $1, error = $2, output = $3
             WHERE id = $4",
        )
        .bind(status)
        .bind(&error)
        .bind(&output)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE scheduled_jobs SET last_status = $1
             WHERE id = (SELECT job_id FROM job_runs WHERE id = $2)",
        )
        .bind(status)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MemoryJobStore — test-only
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MemoryJobStore {
    inner: Mutex<MemInner>,
}

#[derive(Default)]
struct MemInner {
    next_id: i64,
    next_run_id: i64,
    jobs: Vec<JobRecord>,
    runs: Vec<MemRun>,
}

struct MemRun {
    id: i64,
    job_id: i64,
    status: String,
    error: Option<String>,
    output: Option<serde_json::Value>,
}

impl MemoryJobStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn run_status(&self, run_id: i64) -> Option<String> {
        let g = self.inner.lock().expect("memory job store poisoned");
        g.runs.iter().find(|r| r.id == run_id).map(|r| r.status.clone())
    }

    pub fn job_last_status(&self, job_id: i64) -> Option<String> {
        let g = self.inner.lock().expect("memory job store poisoned");
        g.runs
            .iter()
            .rev()
            .find(|r| r.job_id == job_id && r.status != "running")
            .map(|r| r.status.clone())
    }
}

#[async_trait]
impl JobStore for MemoryJobStore {
    async fn upsert(&self, job: NewJob) -> SchedulerResult<JobRecord> {
        let mut g = self.inner.lock().expect("memory job store poisoned");
        if let Some(existing) = g.jobs.iter_mut().find(|j| j.name == job.name) {
            existing.schedule_kind = job.schedule_kind;
            existing.schedule_expr = job.schedule_expr;
            existing.handler = job.handler;
            existing.payload = job.payload;
            existing.timeout_s = job.timeout_s;
            existing.max_retries = job.max_retries;
            return Ok(existing.clone());
        }
        g.next_id += 1;
        let rec = JobRecord {
            id: g.next_id,
            name: job.name,
            schedule_kind: job.schedule_kind,
            schedule_expr: job.schedule_expr,
            handler: job.handler,
            payload: job.payload,
            enabled: true,
            timeout_s: job.timeout_s,
            max_retries: job.max_retries,
            next_run_at: Utc::now(),
        };
        g.jobs.push(rec.clone());
        Ok(rec)
    }

    async fn lease_due(
        &self,
        now: DateTime<Utc>,
        _worker_id: &str,
    ) -> SchedulerResult<Option<(JobRecord, i64)>> {
        let mut g = self.inner.lock().expect("memory job store poisoned");
        let idx = g
            .jobs
            .iter()
            .position(|j| j.enabled && j.next_run_at <= now);
        let Some(i) = idx else { return Ok(None) };
        let job = g.jobs[i].clone();
        let schedule = Schedule::parse(&job.schedule_kind, &job.schedule_expr)?;
        let next = next_after(&schedule, now)?;
        g.jobs[i].next_run_at = next;
        g.next_run_id += 1;
        let run_id = g.next_run_id;
        g.runs.push(MemRun {
            id: run_id,
            job_id: job.id,
            status: "running".to_string(),
            error: None,
            output: None,
        });
        Ok(Some((job, run_id)))
    }

    async fn finish_run(&self, run_id: i64, outcome: RunOutcome) -> SchedulerResult<()> {
        let mut g = self.inner.lock().expect("memory job store poisoned");
        if let Some(r) = g.runs.iter_mut().find(|r| r.id == run_id) {
            match outcome {
                RunOutcome::Success(out) => {
                    r.status = "success".into();
                    r.output = Some(out);
                }
                RunOutcome::Failed(msg) => {
                    r.status = "failed".into();
                    r.error = Some(msg);
                }
                RunOutcome::Timeout => {
                    r.status = "timeout".into();
                    r.error = Some("handler timed out".into());
                }
            }
        }
        Ok(())
    }
}
