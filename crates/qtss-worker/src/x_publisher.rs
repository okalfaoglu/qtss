//! Faz 9.7.6 — `x_outbox` publisher loop.
//!
//! Drains pending rows, posts them via the shared
//! [`NotificationDispatcher`] (X channel uses Bearer token under the
//! hood), enforces a config-driven daily cap, and stamps delivery
//! results. All knobs go through `qtss_config` (CLAUDE.md #2).

use std::time::Duration;

use qtss_notify::{Notification, NotificationChannel, NotificationDispatcher};
use qtss_storage::{
    claim_x_outbox_batch, count_sent_today_utc, mark_x_failed, mark_x_sent,
    resolve_system_u64, resolve_worker_enabled_flag, resolve_worker_tick_secs, XOutboxRow,
};
use sqlx::PgPool;
use tracing::{debug, info, warn};

const MODULE: &str = "notify";
const MAX_ATTEMPTS: i16 = 5;

async fn tick_once(
    pool: &PgPool,
    dispatcher: &NotificationDispatcher,
    batch_limit: i64,
    daily_cap: u64,
) -> Result<usize, sqlx::Error> {
    // Daily cap — short-circuit before claiming rows.
    let sent_today = count_sent_today_utc(pool).await.unwrap_or(0) as u64;
    if sent_today >= daily_cap {
        debug!(sent_today, daily_cap, "x publisher: daily cap reached");
        return Ok(0);
    }
    let remaining = (daily_cap - sent_today) as i64;
    let claim_n = batch_limit.min(remaining);
    if claim_n <= 0 {
        return Ok(0);
    }

    let rows = match claim_x_outbox_batch(pool, claim_n).await {
        Ok(v) => v,
        Err(e) => {
            warn!(%e, "x publisher: claim failed");
            return Ok(0);
        }
    };
    let mut sent = 0usize;
    for row in rows {
        publish_one(pool, dispatcher, &row).await;
        sent += 1;
    }
    Ok(sent)
}

async fn publish_one(
    pool: &PgPool,
    dispatcher: &NotificationDispatcher,
    row: &XOutboxRow,
) {
    let n = Notification::new(row.event_key.clone(), row.body.clone());
    match dispatcher.send(NotificationChannel::X, &n).await {
        Ok(rec) => {
            let tweet_id = rec.provider_id.as_deref().unwrap_or("");
            let permalink = rec
                .provider_id
                .as_deref()
                .map(|id| format!("https://x.com/i/web/status/{id}"));
            if let Err(e) =
                mark_x_sent(pool, row.id, tweet_id, permalink.as_deref()).await
            {
                warn!(%e, id=%row.id, "x publisher: mark_x_sent failed");
            } else {
                debug!(id=%row.id, tweet_id, "x publisher: sent");
            }
        }
        Err(e) => {
            let terminal = row.attempt_count + 1 >= MAX_ATTEMPTS;
            let err = e.to_string();
            warn!(%err, id=%row.id, attempt=row.attempt_count+1, terminal,
                  "x publisher: send failed");
            if let Err(e2) = mark_x_failed(pool, row.id, &err, terminal).await {
                warn!(%e2, id=%row.id, "x publisher: mark_x_failed failed");
            }
        }
    }
}

pub async fn x_publisher_loop(pool: PgPool) {
    info!("x_publisher loop spawned");
    let dispatcher = NotificationDispatcher::from_env();
    loop {
        let enabled = resolve_worker_enabled_flag(
            &pool,
            MODULE,
            "x.publisher.enabled",
            "QTSS_NOTIFY_X_PUBLISHER_ENABLED",
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
            "x.publisher.tick_secs",
            "QTSS_NOTIFY_X_PUBLISHER_TICK_SECS",
            10,
            2,
        )
        .await;
        let batch_limit = resolve_system_u64(
            &pool,
            MODULE,
            "x.publisher.batch_limit",
            "QTSS_NOTIFY_X_PUBLISHER_BATCH",
            10,
            1,
            100,
        )
        .await as i64;
        let daily_cap = resolve_system_u64(
            &pool,
            MODULE,
            "x.publisher.daily_cap",
            "QTSS_NOTIFY_X_PUBLISHER_DAILY_CAP",
            50,
            1,
            10_000,
        )
        .await;

        match tick_once(&pool, &dispatcher, batch_limit, daily_cap).await {
            Ok(0) => {}
            Ok(n) => debug!(n, "x publisher: drained"),
            Err(e) => warn!(%e, "x publisher: tick error"),
        }
        tokio::time::sleep(Duration::from_secs(tick_secs)).await;
    }
}
