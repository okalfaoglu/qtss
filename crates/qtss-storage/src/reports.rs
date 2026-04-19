//! Faz 9C — market-wide periodic performance reports.
//!
//! Daily digest is per-user; weekly / monthly / yearly is market-wide
//! and flows through Telegram + X outbox. Each report is idempotent
//! via the (kind, window_start) unique index on `qtss_reports_runs`.

use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::StorageError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportKind {
    Weekly,
    Monthly,
    Yearly,
}

impl ReportKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ReportKind::Weekly => "weekly",
            ReportKind::Monthly => "monthly",
            ReportKind::Yearly => "yearly",
        }
    }
}

/// Aggregate payload for the report body. Kept flat on purpose so the
/// renderer can produce both Telegram HTML and a 280-char X post
/// without reshaping.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReportAggregate {
    pub kind: String, // ReportKind::as_str()
    pub window_start_utc: DateTime<Utc>,
    pub window_end_utc: DateTime<Utc>,
    pub opened: i64,
    pub closed: i64,
    pub tp_final: i64,
    pub sl_hit: i64,
    pub invalidated: i64,
    pub cancelled: i64,
    pub total_pnl_pct: f64,
    /// Closed setups with a realized-pnl row; used to derive win_rate
    /// from tp_final / wins.
    pub closed_with_pnl: i64,
    pub avg_pnl_pct: f64,
    /// tp_final / (tp_final + sl_hit), 0.0 when denominator is zero.
    pub win_rate: f64,
}

impl ReportAggregate {
    pub fn win_rate_pct(&self) -> f64 {
        self.win_rate * 100.0
    }
}

/// Compute the market-wide aggregate for a UTC window. Reuses the
/// same `qtss_setups` columns that daily digest walks — closed_at,
/// close_reason, realized_pnl_pct — so one SQL is enough.
pub async fn aggregate_report(
    pool: &PgPool,
    kind: ReportKind,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<ReportAggregate, StorageError> {
    // One round-trip for the whole payload.
    let row: (i64, i64, i64, i64, i64, i64, f64, i64, f64) = sqlx::query_as(
        r#"
        SELECT
          COUNT(*) FILTER (WHERE s.created_at >= $1 AND s.created_at < $2)         AS opened,
          COUNT(*) FILTER (WHERE s.closed_at  >= $1 AND s.closed_at  < $2)         AS closed,
          COUNT(*) FILTER (WHERE s.close_reason = 'tp_final'    AND s.closed_at >= $1 AND s.closed_at < $2) AS tp_final,
          COUNT(*) FILTER (WHERE s.close_reason = 'sl_hit'      AND s.closed_at >= $1 AND s.closed_at < $2) AS sl_hit,
          COUNT(*) FILTER (WHERE s.close_reason = 'invalidated' AND s.closed_at >= $1 AND s.closed_at < $2) AS invalidated,
          COUNT(*) FILTER (WHERE s.close_reason = 'cancelled'   AND s.closed_at >= $1 AND s.closed_at < $2) AS cancelled,
          COALESCE(SUM(s.realized_pnl_pct) FILTER (WHERE s.closed_at >= $1 AND s.closed_at < $2), 0.0)::float8 AS total_pnl_pct,
          COUNT(*) FILTER (WHERE s.realized_pnl_pct IS NOT NULL AND s.closed_at >= $1 AND s.closed_at < $2) AS closed_with_pnl,
          COALESCE(AVG(s.realized_pnl_pct) FILTER (WHERE s.realized_pnl_pct IS NOT NULL AND s.closed_at >= $1 AND s.closed_at < $2), 0.0)::float8 AS avg_pnl_pct
          FROM qtss_setups s
        "#,
    )
    .bind(from)
    .bind(to)
    .fetch_one(pool)
    .await?;

    let (opened, closed, tp_final, sl_hit, invalidated, cancelled, total_pnl, with_pnl, avg_pnl) = row;
    let denom = tp_final + sl_hit;
    let win_rate = if denom > 0 { tp_final as f64 / denom as f64 } else { 0.0 };

    Ok(ReportAggregate {
        kind: kind.as_str().into(),
        window_start_utc: from,
        window_end_utc: to,
        opened,
        closed,
        tp_final,
        sl_hit,
        invalidated,
        cancelled,
        total_pnl_pct: total_pnl,
        closed_with_pnl: with_pnl,
        avg_pnl_pct: avg_pnl,
        win_rate,
    })
}

/// Returns true iff a report for (kind, window_start) has already been
/// recorded. The scheduler calls this guard before generating, so a
/// worker restart inside the dispatch hour won't double-send.
pub async fn report_exists(
    pool: &PgPool,
    kind: ReportKind,
    window_start: DateTime<Utc>,
) -> Result<bool, StorageError> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM qtss_reports_runs WHERE kind = $1 AND window_start = $2",
    )
    .bind(kind.as_str())
    .bind(window_start)
    .fetch_one(pool)
    .await?;
    Ok(n > 0)
}

#[derive(Debug, Clone)]
pub struct ReportRunInsert {
    pub kind: ReportKind,
    pub window_start: DateTime<Utc>,
    pub window_end: DateTime<Utc>,
    pub telegram_ok: Option<bool>,
    pub x_ok: Option<bool>,
    pub body_telegram: Option<String>,
    pub body_x: Option<String>,
    pub aggregate_json: JsonValue,
    pub last_error: Option<String>,
}

pub async fn record_report_run(
    pool: &PgPool,
    row: &ReportRunInsert,
) -> Result<Uuid, StorageError> {
    let id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO qtss_reports_runs (
            kind, window_start, window_end,
            telegram_ok, x_ok, body_telegram, body_x,
            aggregate_json, last_error
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        ON CONFLICT (kind, window_start) DO UPDATE SET
            window_end     = EXCLUDED.window_end,
            telegram_ok    = EXCLUDED.telegram_ok,
            x_ok           = EXCLUDED.x_ok,
            body_telegram  = EXCLUDED.body_telegram,
            body_x         = EXCLUDED.body_x,
            aggregate_json = EXCLUDED.aggregate_json,
            last_error     = EXCLUDED.last_error,
            generated_at   = NOW()
        RETURNING id
        "#,
    )
    .bind(row.kind.as_str())
    .bind(row.window_start)
    .bind(row.window_end)
    .bind(row.telegram_ok)
    .bind(row.x_ok)
    .bind(&row.body_telegram)
    .bind(&row.body_x)
    .bind(&row.aggregate_json)
    .bind(&row.last_error)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

// ── Window helpers ──────────────────────────────────────────────────
//
// Each helper returns (window_start, window_end) for the *previous*
// completed period relative to `now_utc`. The scheduler fires a report
// for this window once `now_utc >= window_end` + dispatch_hour.

/// Previous ISO week (Mon 00:00 UTC → next Mon 00:00 UTC).
pub fn previous_week_window(now_utc: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    // days since Monday (0 = Mon). weekday().num_days_from_monday() handles that.
    let days_since_mon = now_utc.weekday().num_days_from_monday() as i64;
    let this_week_start_date = (now_utc.date_naive()) - chrono::Duration::days(days_since_mon);
    let prev_week_start_date = this_week_start_date - chrono::Duration::days(7);
    let start = Utc
        .from_utc_datetime(&prev_week_start_date.and_hms_opt(0, 0, 0).unwrap());
    let end = Utc.from_utc_datetime(&this_week_start_date.and_hms_opt(0, 0, 0).unwrap());
    (start, end)
}

/// Previous calendar month.
pub fn previous_month_window(now_utc: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let y = now_utc.year();
    let m = now_utc.month();
    let this_month_start = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
    let prev_month_start = if m == 1 {
        NaiveDate::from_ymd_opt(y - 1, 12, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(y, m - 1, 1).unwrap()
    };
    let start = Utc.from_utc_datetime(&prev_month_start.and_hms_opt(0, 0, 0).unwrap());
    let end = Utc.from_utc_datetime(&this_month_start.and_hms_opt(0, 0, 0).unwrap());
    (start, end)
}

/// Previous calendar year.
pub fn previous_year_window(now_utc: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let y = now_utc.year();
    let start = Utc.from_utc_datetime(
        &NaiveDate::from_ymd_opt(y - 1, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap(),
    );
    let end = Utc.from_utc_datetime(
        &NaiveDate::from_ymd_opt(y, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap(),
    );
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekly_window_wraps_to_previous_iso_week() {
        // Monday 2026-04-20 10:00 UTC → prev week = [2026-04-13, 2026-04-20).
        let now = Utc.with_ymd_and_hms(2026, 4, 20, 10, 0, 0).unwrap();
        let (s, e) = previous_week_window(now);
        assert_eq!(s, Utc.with_ymd_and_hms(2026, 4, 13, 0, 0, 0).unwrap());
        assert_eq!(e, Utc.with_ymd_and_hms(2026, 4, 20, 0, 0, 0).unwrap());
    }

    #[test]
    fn monthly_window_handles_january_rollover() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap();
        let (s, e) = previous_month_window(now);
        assert_eq!(s, Utc.with_ymd_and_hms(2025, 12, 1, 0, 0, 0).unwrap());
        assert_eq!(e, Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap());
    }

    #[test]
    fn yearly_window_is_prior_calendar_year() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap();
        let (s, e) = previous_year_window(now);
        assert_eq!(s, Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap());
        assert_eq!(e, Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap());
    }
}
