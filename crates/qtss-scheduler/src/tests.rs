use crate::handler::{Handler, HandlerRegistry, HandlerResult};
use crate::schedule::{next_after, Schedule};
use crate::store::{JobStore, MemoryJobStore, NewJob, RunOutcome};
use crate::SchedulerError;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::Arc;

#[test]
fn parses_interval_expressions() {
    assert_eq!(
        Schedule::parse("interval", "30s").unwrap(),
        Schedule::Interval(Duration::seconds(30))
    );
    assert_eq!(
        Schedule::parse("interval", "5m").unwrap(),
        Schedule::Interval(Duration::minutes(5))
    );
    assert_eq!(
        Schedule::parse("interval", "2h").unwrap(),
        Schedule::Interval(Duration::hours(2))
    );
}

#[test]
fn rejects_invalid_intervals() {
    assert!(matches!(
        Schedule::parse("interval", ""),
        Err(SchedulerError::InvalidSchedule(_))
    ));
    assert!(matches!(
        Schedule::parse("interval", "0s"),
        Err(SchedulerError::InvalidSchedule(_))
    ));
    assert!(matches!(
        Schedule::parse("interval", "12x"),
        Err(SchedulerError::InvalidSchedule(_))
    ));
    assert!(matches!(
        Schedule::parse("hourly", "1"),
        Err(SchedulerError::InvalidSchedule(_))
    ));
}

#[test]
fn next_after_advances_by_interval() {
    let now = Utc::now();
    let next = next_after(&Schedule::Interval(Duration::seconds(45)), now).unwrap();
    assert_eq!(next - now, Duration::seconds(45));
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

struct EchoHandler;

#[async_trait]
impl Handler for EchoHandler {
    async fn run(&self, payload: serde_json::Value) -> crate::SchedulerResult<HandlerResult> {
        Ok(HandlerResult { output: payload })
    }
}

#[test]
fn registry_dispatches_by_key() {
    let mut reg = HandlerRegistry::new();
    reg.register("echo", Arc::new(EchoHandler));
    assert!(reg.get("echo").is_ok());
    assert!(matches!(
        reg.get("missing"),
        Err(SchedulerError::UnknownHandler(_))
    ));
}

// ---------------------------------------------------------------------------
// Memory job store + lease/finish round trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn upsert_then_lease_then_finish_success() {
    let store = MemoryJobStore::new();
    let job = store
        .upsert(NewJob {
            name: "nansen.smart_money_pull".into(),
            description: Some("pull SM flows".into()),
            schedule_kind: "interval".into(),
            schedule_expr: "30s".into(),
            handler: "nansen.pull".into(),
            payload: serde_json::json!({ "limit": 100 }),
            timeout_s: 60,
            max_retries: 3,
        })
        .await
        .unwrap();

    let now = Utc::now();
    let leased = store.lease_due(now, "worker-1").await.unwrap();
    let (rec, run_id) = leased.expect("a job should be due");
    assert_eq!(rec.id, job.id);
    assert_eq!(store.run_status(run_id).as_deref(), Some("running"));

    // Second lease at the same instant must return None — next_run_at advanced.
    assert!(store.lease_due(now, "worker-1").await.unwrap().is_none());

    store
        .finish_run(run_id, RunOutcome::Success(serde_json::json!({ "rows": 42 })))
        .await
        .unwrap();
    assert_eq!(store.run_status(run_id).as_deref(), Some("success"));
    assert_eq!(store.job_last_status(job.id).as_deref(), Some("success"));
}

#[tokio::test]
async fn finish_run_records_failure() {
    let store = MemoryJobStore::new();
    store
        .upsert(NewJob {
            name: "j".into(),
            description: None,
            schedule_kind: "interval".into(),
            schedule_expr: "10s".into(),
            handler: "noop".into(),
            payload: serde_json::json!({}),
            timeout_s: 5,
            max_retries: 0,
        })
        .await
        .unwrap();
    let (_, run_id) = store
        .lease_due(Utc::now(), "w")
        .await
        .unwrap()
        .expect("due");
    store
        .finish_run(run_id, RunOutcome::Failed("boom".into()))
        .await
        .unwrap();
    assert_eq!(store.run_status(run_id).as_deref(), Some("failed"));
}

#[tokio::test]
async fn lease_skips_when_nothing_due() {
    let store = MemoryJobStore::new();
    assert!(store.lease_due(Utc::now(), "w").await.unwrap().is_none());
}
