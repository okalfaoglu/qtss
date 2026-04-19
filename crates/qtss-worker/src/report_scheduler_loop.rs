//! Faz 9C — market-wide periodic report scheduler (weekly / monthly /
//! yearly). Fires at a configured UTC dispatch hour on the appropriate
//! boundary, aggregates closed setups for the previous completed
//! window, renders a Telegram HTML body + an X post body, dispatches
//! both channels, and stamps `qtss_reports_runs` so restarts don't
//! double-send.
//!
//! CLAUDE.md:
//!   * #1 — decision of "which report kind is due right now" is a
//!     dispatch-style filter over a fixed array of (kind, is_due, window)
//!     triples, not a nested if/else.
//!   * #2 — every knob is in `system_config` under module=`notify`.

use std::time::Duration;

use chrono::{Datelike, Timelike, Utc};
use qtss_notify::{NotificationChannel, NotificationDispatcher};
use qtss_storage::{
    aggregate_report, enqueue_x_outbox, previous_month_window, previous_week_window,
    previous_year_window, record_report_run, report_exists, resolve_system_u64,
    resolve_worker_enabled_flag, resolve_worker_tick_secs, ReportAggregate, ReportKind,
    ReportRunInsert, XOutboxInsert,
};
use sqlx::PgPool;
use tracing::{debug, info, warn};

const MODULE: &str = "notify";

/// A kind becomes due when `now_utc` is at/past the dispatch hour on
/// the boundary day (Mon for weekly, 1st for monthly, Jan-1 for yearly)
/// AND the current window hasn't been stamped yet.
fn boundary_hit(now: chrono::DateTime<Utc>, dispatch_hour: u32, kind: ReportKind) -> bool {
    if now.hour() < dispatch_hour {
        return false;
    }
    match kind {
        ReportKind::Weekly => now.weekday() == chrono::Weekday::Mon,
        ReportKind::Monthly => now.day() == 1,
        ReportKind::Yearly => now.month() == 1 && now.day() == 1,
    }
}

fn window_for(now: chrono::DateTime<Utc>, kind: ReportKind) -> (chrono::DateTime<Utc>, chrono::DateTime<Utc>) {
    match kind {
        ReportKind::Weekly => previous_week_window(now),
        ReportKind::Monthly => previous_month_window(now),
        ReportKind::Yearly => previous_year_window(now),
    }
}

// ── Render ─────────────────────────────────────────────────────────
//
// Kept inline here (short, self-contained) rather than threading a
// sixth renderer into qtss-notify. Telegram gets HTML; X gets a flat
// <= 280-char line. Turkish, per CLAUDE.md #6.

fn label(kind: ReportKind) -> &'static str {
    match kind {
        ReportKind::Weekly => "Haftalık",
        ReportKind::Monthly => "Aylık",
        ReportKind::Yearly => "Yıllık",
    }
}

fn fmt_window(a: &ReportAggregate) -> String {
    format!(
        "{}–{}",
        a.window_start_utc.format("%Y-%m-%d"),
        a.window_end_utc.format("%Y-%m-%d")
    )
}

fn render_telegram(kind: ReportKind, a: &ReportAggregate) -> String {
    let arrow = if a.total_pnl_pct >= 0.0 { "📈" } else { "📉" };
    format!(
        "<b>{} QTSS Raporu</b>\n{}\n\n\
         Açılan: <b>{}</b>\n\
         Kapanan: <b>{}</b> (TP {} / SL {} / iptal {} / geçersiz {})\n\
         Kazanç oranı: <b>{:.1}%</b>\n\
         Toplam PnL: {} <b>{:+.2}%</b>\n\
         Ortalama PnL: <b>{:+.2}%</b>",
        label(kind),
        fmt_window(a),
        a.opened,
        a.closed,
        a.tp_final,
        a.sl_hit,
        a.cancelled,
        a.invalidated,
        a.win_rate_pct(),
        arrow,
        a.total_pnl_pct,
        a.avg_pnl_pct,
    )
}

fn render_x(kind: ReportKind, a: &ReportAggregate) -> String {
    // 280-char budget. Skip channel-specific ornaments; include a hashtag.
    let body = format!(
        "QTSS {} Rapor {} | Açılan {} Kapanan {} | Win {:.1}% | PnL {:+.2}% (ort {:+.2}%) #QTSS",
        label(kind),
        fmt_window(a),
        a.opened,
        a.closed,
        a.win_rate_pct(),
        a.total_pnl_pct,
        a.avg_pnl_pct,
    );
    // Hard cap — truncate on a char boundary if longer.
    if body.chars().count() <= 280 {
        body
    } else {
        body.chars().take(277).chain("...".chars()).collect()
    }
}

// ── One tick ───────────────────────────────────────────────────────

async fn dispatch_one(
    pool: &PgPool,
    dispatcher: &NotificationDispatcher,
    kind: ReportKind,
    now: chrono::DateTime<Utc>,
    send_telegram: bool,
    send_x: bool,
) -> Result<bool, sqlx::Error> {
    let (from, to) = window_for(now, kind);
    // Idempotence guard.
    match report_exists(pool, kind, from).await {
        Ok(true) => return Ok(false),
        Ok(false) => {}
        Err(e) => {
            warn!(%e, kind = kind.as_str(), "report_exists check failed");
            return Ok(false);
        }
    }

    let agg = match aggregate_report(pool, kind, from, to).await {
        Ok(a) => a,
        Err(e) => {
            warn!(%e, kind = kind.as_str(), "aggregate_report failed");
            return Ok(false);
        }
    };

    let tg_body = render_telegram(kind, &agg);
    let x_body = render_x(kind, &agg);

    // Telegram dispatch (soft-fail).
    let telegram_ok = if send_telegram {
        let title = format!("QTSS {} raporu", label(kind));
        let n = qtss_notify::Notification::new(title, tg_body.clone())
            .with_telegram_html_message(tg_body.clone());
        match dispatcher.send(NotificationChannel::Telegram, &n).await {
            Ok(_) => Some(true),
            Err(e) => {
                warn!(%e, kind = kind.as_str(), "telegram report send failed");
                Some(false)
            }
        }
    } else {
        None
    };

    // X outbox enqueue.
    let x_ok = if send_x {
        let ins = XOutboxInsert {
            setup_id: None,
            lifecycle_event_id: None,
            event_key: format!("report.{}", kind.as_str()),
            body: x_body.clone(),
            image_path: None,
        };
        match enqueue_x_outbox(pool, &ins).await {
            Ok(_) => Some(true),
            Err(e) => {
                warn!(%e, kind = kind.as_str(), "x_outbox enqueue failed");
                Some(false)
            }
        }
    } else {
        None
    };

    // Record (upserts on conflict to survive flaky retries).
    let agg_json = serde_json::to_value(&agg).unwrap_or(serde_json::json!({}));
    let row = ReportRunInsert {
        kind,
        window_start: from,
        window_end: to,
        telegram_ok,
        x_ok,
        body_telegram: Some(tg_body),
        body_x: Some(x_body),
        aggregate_json: agg_json,
        last_error: None,
    };
    if let Err(e) = record_report_run(pool, &row).await {
        warn!(%e, kind = kind.as_str(), "record_report_run failed");
    }
    info!(
        kind = kind.as_str(),
        window = %fmt_window(&agg),
        opened = agg.opened,
        closed = agg.closed,
        win_rate_pct = agg.win_rate_pct(),
        total_pnl_pct = agg.total_pnl_pct,
        "periodic report dispatched"
    );
    Ok(true)
}

// ── Loop ───────────────────────────────────────────────────────────

pub async fn report_scheduler_loop(pool: PgPool) {
    info!("report_scheduler loop spawned");
    let dispatcher = NotificationDispatcher::from_env();
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool, MODULE, "report.enabled", "QTSS_NOTIFY_REPORT_ENABLED", false,
        )
        .await;
        if !enabled {
            tokio::time::sleep(Duration::from_secs(120)).await;
            continue;
        }
        let tick_secs = resolve_worker_tick_secs(
            &pool, MODULE, "report.scan_tick_secs", "QTSS_NOTIFY_REPORT_SCAN_SECS", 300, 60,
        )
        .await;
        let dispatch_hour = resolve_system_u64(
            &pool, MODULE, "report.dispatch_hour_utc", "QTSS_NOTIFY_REPORT_HOUR_UTC",
            9, 0, 23,
        )
        .await as u32;

        let weekly_on = resolve_worker_enabled_flag(
            &pool, MODULE, "report.weekly_enabled", "", true,
        ).await;
        let monthly_on = resolve_worker_enabled_flag(
            &pool, MODULE, "report.monthly_enabled", "", true,
        ).await;
        let yearly_on = resolve_worker_enabled_flag(
            &pool, MODULE, "report.yearly_enabled", "", true,
        ).await;
        let send_tg = resolve_worker_enabled_flag(
            &pool, MODULE, "report.send_telegram", "", true,
        ).await;
        let send_x = resolve_worker_enabled_flag(
            &pool, MODULE, "report.send_x", "", true,
        ).await;

        let now = Utc::now();
        // Dispatch table — order matters only for log readability.
        let candidates: [(ReportKind, bool); 3] = [
            (ReportKind::Yearly, yearly_on),
            (ReportKind::Monthly, monthly_on),
            (ReportKind::Weekly, weekly_on),
        ];
        for (kind, on) in candidates {
            if !on {
                continue;
            }
            if !boundary_hit(now, dispatch_hour, kind) {
                debug!(kind = kind.as_str(), "not a boundary hit — skip");
                continue;
            }
            if let Err(e) = dispatch_one(&pool, &dispatcher, kind, now, send_tg, send_x).await {
                warn!(%e, kind = kind.as_str(), "dispatch_one errored");
            }
        }

        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
    }
}
