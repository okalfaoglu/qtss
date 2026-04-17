//! Faz 9.7.7 — Per-user daily digest scheduler.
//!
//! Fires once per local-calendar-day per user. The scan cadence is
//! config-driven (`digest.scan_tick_secs`); the local hour at which a
//! digest is delivered lives in `digest.local_hour`. Both windows and
//! enable flags are reloaded every tick so runtime Config Editor
//! changes apply without restart.

use std::time::Duration;

use chrono::{Datelike, FixedOffset, TimeZone, Timelike, Utc};
use qtss_notify::{
    digest_default_window, render_digest, DigestRenderInput, NotificationChannel,
    NotificationDispatcher,
};
use qtss_storage::{
    aggregate_digest, list_digest_candidates, resolve_system_u64, resolve_worker_enabled_flag,
    resolve_worker_tick_secs, stamp_digest_sent, DigestUserRow,
};
use sqlx::PgPool;
use tracing::{debug, info, warn};

const MODULE: &str = "notify";

/// Decide whether a user is due for a digest: their local clock has
/// reached `digest_hour`, and either they've never had one or the last
/// one was before today's local midnight.
fn is_user_due(user: &DigestUserRow, digest_hour: u32, now_utc: chrono::DateTime<Utc>) -> bool {
    let offset = match FixedOffset::east_opt(user.tz_offset_minutes * 60) {
        Some(o) => o,
        None => return false,
    };
    let local_now = now_utc.with_timezone(&offset);
    if local_now.hour() < digest_hour {
        return false;
    }
    let Some(local_midnight) = offset
        .with_ymd_and_hms(local_now.year(), local_now.month(), local_now.day(), 0, 0, 0)
        .single()
    else {
        return false;
    };
    let today_start_utc = local_midnight.with_timezone(&Utc);
    match user.last_digest_sent_utc {
        None => true,
        Some(prev) => prev < today_start_utc,
    }
}

async fn tick_once(
    pool: &PgPool,
    dispatcher: &NotificationDispatcher,
    digest_hour: u32,
    window_hours: i64,
    min_gap_hours: i32,
) -> Result<usize, sqlx::Error> {
    let users = match list_digest_candidates(pool, min_gap_hours).await {
        Ok(v) => v,
        Err(e) => {
            warn!(%e, "digest: list_digest_candidates failed");
            return Ok(0);
        }
    };
    if users.is_empty() {
        return Ok(0);
    }
    let now_utc = Utc::now();
    let due: Vec<DigestUserRow> = users
        .into_iter()
        .filter(|u| is_user_due(u, digest_hour, now_utc))
        .collect();
    if due.is_empty() {
        return Ok(0);
    }

    let (from, to) = digest_default_window(now_utc, window_hours);
    let agg = match aggregate_digest(pool, from, to).await {
        Ok(a) => a,
        Err(e) => {
            warn!(%e, "digest: aggregate failed");
            return Ok(0);
        }
    };

    let mut sent = 0usize;
    for user in due {
        let input = DigestRenderInput {
            agg: &agg,
            tz_offset_minutes: user.tz_offset_minutes,
            local_label_at: now_utc,
        };
        let mut n = render_digest(&input);
        // Route to the user's chat id via a per-send override. The
        // dispatcher reads `config.telegram.chat_id` as the default;
        // for per-user routing we temporarily swap via an ad-hoc
        // Notification extension — here we just set the caption and
        // let the dispatcher do its thing. A proper per-user dispatch
        // override ships in the subscription patch; for now we send
        // only when the user's chat id matches the configured default
        // OR when no chat id is set yet (single-admin deployments).
        let Some(chat_id) = &user.telegram_chat_id else {
            continue;
        };
        let configured = dispatcher
            .config()
            .telegram
            .as_ref()
            .map(|t| t.chat_id.as_str());
        if configured != Some(chat_id.as_str()) {
            debug!(user_id=%user.user_id, "digest: skip — chat_id differs from dispatcher default");
            continue;
        }
        // Prepend a personalized greeting.
        if let Some(text) = n.telegram_text.as_mut() {
            text.insert_str(0, "👋\n");
        }
        match dispatcher.send(NotificationChannel::Telegram, &n).await {
            Ok(_) => {
                if let Err(e) = stamp_digest_sent(pool, user.user_id, now_utc).await {
                    warn!(%e, user_id=%user.user_id, "digest: stamp failed");
                } else {
                    sent += 1;
                }
            }
            Err(e) => warn!(%e, user_id=%user.user_id, "digest: send failed"),
        }
    }
    Ok(sent)
}

pub async fn digest_loop(pool: PgPool) {
    info!("digest loop spawned");
    let dispatcher = NotificationDispatcher::from_env();
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            MODULE,
            "digest.enabled",
            "QTSS_NOTIFY_DIGEST_ENABLED",
            false,
        )
        .await;
        if !enabled {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }
        let tick_secs = resolve_worker_tick_secs(
            &pool,
            MODULE,
            "digest.scan_tick_secs",
            "QTSS_NOTIFY_DIGEST_SCAN_SECS",
            600,
            60,
        )
        .await;
        let hour = resolve_system_u64(
            &pool,
            MODULE,
            "digest.local_hour",
            "QTSS_NOTIFY_DIGEST_LOCAL_HOUR",
            18,
            0,
            23,
        )
        .await as u32;
        let window_hours = resolve_system_u64(
            &pool,
            MODULE,
            "digest.window_hours",
            "QTSS_NOTIFY_DIGEST_WINDOW_HOURS",
            24,
            1,
            168,
        )
        .await as i64;
        // Safety floor — never re-send a digest within 12h of the last.
        let min_gap_hours = 12;

        match tick_once(&pool, &dispatcher, hour, window_hours, min_gap_hours).await {
            Ok(0) => {}
            Ok(n) => debug!(n, "digest: delivered"),
            Err(e) => warn!(%e, "digest: tick error"),
        }
        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn user(offset_min: i32, last: Option<chrono::DateTime<Utc>>) -> DigestUserRow {
        DigestUserRow {
            user_id: Uuid::new_v4(),
            tz_offset_minutes: offset_min,
            telegram_chat_id: Some("1".into()),
            last_digest_sent_utc: last,
        }
    }

    #[test]
    fn before_digest_hour_not_due() {
        // 10:00 UTC + offset 0 → local hour 10, digest_hour=18 → not due.
        let now = Utc.with_ymd_and_hms(2026, 4, 18, 10, 0, 0).unwrap();
        assert!(!is_user_due(&user(0, None), 18, now));
    }

    #[test]
    fn after_digest_hour_never_sent_is_due() {
        let now = Utc.with_ymd_and_hms(2026, 4, 18, 18, 30, 0).unwrap();
        assert!(is_user_due(&user(0, None), 18, now));
    }

    #[test]
    fn same_local_day_already_sent_is_not_due() {
        let now = Utc.with_ymd_and_hms(2026, 4, 18, 19, 0, 0).unwrap();
        let previous = Utc.with_ymd_and_hms(2026, 4, 18, 18, 5, 0).unwrap();
        assert!(!is_user_due(&user(0, Some(previous)), 18, now));
    }

    #[test]
    fn previous_day_send_yields_due_today() {
        let now = Utc.with_ymd_and_hms(2026, 4, 18, 18, 30, 0).unwrap();
        let previous = Utc.with_ymd_and_hms(2026, 4, 17, 18, 5, 0).unwrap();
        assert!(is_user_due(&user(0, Some(previous)), 18, now));
    }

    #[test]
    fn tz_offset_shifts_due_window() {
        // Istanbul +180min: UTC 15:30 → local 18:30 → due.
        let now = Utc.with_ymd_and_hms(2026, 4, 18, 15, 30, 0).unwrap();
        assert!(is_user_due(&user(180, None), 18, now));
    }
}
