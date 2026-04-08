//! qtss-scheduler — recurring task runner backed by Postgres.
//!
//! The scheduler is responsible for periodically waking up registered
//! handlers (Nansen pulls, on-chain feeds, archive jobs, ...). The job
//! catalog and run history live in the DB so multiple scheduler workers
//! can cooperate without external coordination.
//!
//! ## How it runs
//!
//! Each tick (configurable, default 5s) the scheduler:
//!   1. SELECTs due jobs FOR UPDATE SKIP LOCKED.
//!   2. For each, INSERTs a `running` row in job_runs and bumps
//!      `next_run_at` based on the job's schedule expression.
//!   3. Looks up the handler in its [`HandlerRegistry`] and calls it.
//!   4. Updates the job_run row with success/failure + the handler output.
//!
//! Handlers are dispatched through a `HashMap<String, Box<dyn Handler>>`
//! — adding a new periodic task = registering one handler, no `if/else`
//! soup in the dispatcher.

mod error;
mod handler;
mod schedule;
mod store;

#[cfg(test)]
mod tests;

pub use error::{SchedulerError, SchedulerResult};
pub use handler::{Handler, HandlerRegistry, HandlerResult};
pub use schedule::{next_after, Schedule};
pub use store::{JobRecord, JobStore, MemoryJobStore, NewJob, PgJobStore, RunOutcome};
