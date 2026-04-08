//! Schedule expression parsing and "next fire time" calculation.
//!
//! Two kinds:
//!   * `interval:<duration>`  — fixed delay between runs (e.g. `interval:30s`,
//!                              `interval:5m`, `interval:2h`).
//!   * `cron:<expr>`          — full cron expression. Cron is intentionally
//!                              parked behind a stub for the skeleton PR;
//!                              real parsing lands when the first cron-driven
//!                              job (archive rollups) is added.
//!
//! Keeping kind dispatch as a small enum + match instead of branching in
//! the scheduler core means adding a new schedule kind is one new variant
//! and one new arm — no scattered `if`s.

use crate::error::{SchedulerError, SchedulerResult};
use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    Interval(Duration),
    /// Raw cron expression. Parsing happens at fire time.
    Cron(String),
}

impl Schedule {
    /// Parse `(kind, expr)` as stored in `scheduled_jobs`.
    pub fn parse(kind: &str, expr: &str) -> SchedulerResult<Self> {
        match kind {
            "interval" => parse_interval(expr).map(Schedule::Interval),
            "cron" => Ok(Schedule::Cron(expr.to_string())),
            other => Err(SchedulerError::InvalidSchedule(format!(
                "unknown kind '{other}'"
            ))),
        }
    }
}

/// Compute the next fire time after `now` for a given schedule.
pub fn next_after(schedule: &Schedule, now: DateTime<Utc>) -> SchedulerResult<DateTime<Utc>> {
    match schedule {
        Schedule::Interval(d) => Ok(now + *d),
        // Cron parsing is deferred until the first cron job is registered;
        // until then we treat any cron expression as "fire in 60s" so that
        // a misconfigured row cannot wedge the whole scheduler at startup.
        Schedule::Cron(_) => Ok(now + Duration::seconds(60)),
    }
}

fn parse_interval(expr: &str) -> SchedulerResult<Duration> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Err(SchedulerError::InvalidSchedule("empty interval".into()));
    }
    let (num, unit) = expr.split_at(expr.len() - 1);
    let n: i64 = num
        .parse()
        .map_err(|_| SchedulerError::InvalidSchedule(format!("bad number in '{expr}'")))?;
    let dur = match unit {
        "s" => Duration::seconds(n),
        "m" => Duration::minutes(n),
        "h" => Duration::hours(n),
        "d" => Duration::days(n),
        other => {
            return Err(SchedulerError::InvalidSchedule(format!(
                "unknown unit '{other}' (use s|m|h|d)"
            )))
        }
    };
    if dur <= Duration::zero() {
        return Err(SchedulerError::InvalidSchedule(
            "interval must be positive".into(),
        ));
    }
    Ok(dur)
}
