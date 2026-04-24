//! Faz 9.7.3 — Built-in lifecycle handlers.
//!
//! [`DbPersistHandler`] is the default handler: every event is written
//! to `qtss_setup_lifecycle_events` (audit trail) and — for terminal
//! events — the setup row gets `closed_at` / `close_reason` / PnL
//! stamped. Later Faz patches add Telegram and X handlers; this one
//! stays the "source of truth" regardless of downstream channels.
//!
//! CLAUDE.md #3 — keeps DB I/O out of the pure detector; handlers are
//! the only place side-effects happen.

use async_trait::async_trait;
use qtss_storage::{
    close_setup, insert_lifecycle_event, mark_entry_touched, set_tp_hit_bit,
    LifecycleEventInsert, SetupCloseUpdate,
};
use sqlx::PgPool;
use tracing::{debug, warn};

use crate::dispatch::NotificationDispatcher;
use crate::lifecycle::{LifecycleContext, LifecycleEventKind, LifecycleHandler};
use crate::telegram_render::render_lifecycle;
use crate::types::NotificationChannel;

pub struct DbPersistHandler {
    pool: PgPool,
}

impl DbPersistHandler {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl LifecycleHandler for DbPersistHandler {
    fn name(&self) -> &'static str {
        "db_persist"
    }

    async fn on_event(&self, ctx: &LifecycleContext) {
        // 1. Audit row — always.
        let insert = LifecycleEventInsert {
            setup_id: ctx.setup_id,
            event_kind: ctx.kind.as_db_str().to_string(),
            price: ctx.price,
            pnl_pct: ctx.pnl_pct,
            pnl_r: ctx.pnl_r,
            health_score: ctx.health.as_ref().map(|h| h.total),
            duration_ms: ctx.duration_ms,
            ai_action: ctx.ai_action.clone(),
            ai_reasoning: ctx.ai_reasoning.clone(),
            ai_confidence: ctx.ai_confidence,
        };
        if let Err(e) = insert_lifecycle_event(&self.pool, &insert).await {
            warn!(%e, setup_id=%ctx.setup_id, kind=%ctx.kind.as_db_str(),
                  "DbPersistHandler: insert_lifecycle_event");
            // Keep going — we still want to try the setup-row update if
            // this is a terminal event, so the user's portfolio view
            // stays consistent even if the audit write hiccuped.
        }

        // 2. Side-tables per kind — small dispatch table.
        match ctx.kind {
            LifecycleEventKind::EntryTouched => {
                if let Err(e) = mark_entry_touched(&self.pool, ctx.setup_id, ctx.emitted_at).await {
                    warn!(%e, setup_id=%ctx.setup_id, "mark_entry_touched");
                }
            }
            LifecycleEventKind::TpHit | LifecycleEventKind::TpPartial | LifecycleEventKind::TpFinal => {
                if let Some(idx) = ctx.tp_index {
                    if let Err(e) = set_tp_hit_bit(&self.pool, ctx.setup_id, idx).await {
                        warn!(%e, setup_id=%ctx.setup_id, tp_index=idx, "set_tp_hit_bit");
                    }
                }
            }
            _ => {}
        }

        // 3. Close setup on terminal events.
        if let Some(reason) = ctx.kind.close_reason() {
            let update = SetupCloseUpdate {
                setup_id: ctx.setup_id,
                close_reason: reason.to_string(),
                close_price: ctx.price,
                realized_pnl_pct: ctx.pnl_pct,
                realized_r: ctx.pnl_r,
            };
            if let Err(e) = close_setup(&self.pool, &update).await {
                warn!(%e, setup_id=%ctx.setup_id, %reason, "close_setup");
            } else {
                debug!(setup_id=%ctx.setup_id, %reason, "setup closed");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Telegram lifecycle handler (Faz 9.7.5)
// ---------------------------------------------------------------------------

/// Renders each lifecycle event via `render_lifecycle` and enqueues
/// the rendered card into `notify_outbox` with a stable dedup_key so a
/// worker restart never replays an already-sent event. Falls back to
/// direct dispatcher send when no pool is configured (keeps the
/// legacy code path alive for callers that haven't been updated).
///
/// Dedup scheme — `lifecycle:{setup_id}:{event_kind}` — matches one
/// row per terminal-event-kind per setup. Non-terminal events (TpHit
/// intermediate, SlRatcheted, HealthWarn) can legitimately fire many
/// times and are NOT dedup-gated at this layer (they get dedup_key =
/// NULL so the partial unique index skips them).
pub struct TelegramLifecycleHandler {
    dispatcher: NotificationDispatcher,
    pool: Option<PgPool>,
}

impl TelegramLifecycleHandler {
    pub fn new(dispatcher: NotificationDispatcher) -> Self {
        Self {
            dispatcher,
            pool: None,
        }
    }

    /// PgPool-backed constructor. Preferred in production — routes the
    /// card through notify_outbox so the dedup index blocks restart
    /// replays. Legacy `new()` keeps working for tests and fallback.
    pub fn with_pool(dispatcher: NotificationDispatcher, pool: PgPool) -> Self {
        Self {
            dispatcher,
            pool: Some(pool),
        }
    }
}

#[async_trait]
impl LifecycleHandler for TelegramLifecycleHandler {
    fn name(&self) -> &'static str {
        "telegram_lifecycle"
    }

    async fn on_event(&self, ctx: &LifecycleContext) {
        if self.dispatcher.config().telegram.is_none() {
            return;
        }
        let n = render_lifecycle(ctx);
        // Telegram HTML body lives in `telegram_text` (set by
        // with_telegram_html_message). Non-Telegram subscribers get
        // the plain `body` field.
        let body_html = n
            .telegram_text
            .clone()
            .unwrap_or_else(|| n.body.clone());
        let title = n.title.clone();

        // Pool path: enqueue to notify_outbox with dedup. The
        // notify_outbox_loop drains to Telegram on its own cadence.
        if let Some(pool) = &self.pool {
            // Only terminal (close) events get dedup'd here — repeat
            // sub-events on the same setup are legitimate.
            let dedup_key = if ctx.kind.is_terminal() {
                Some(format!(
                    "lifecycle:{}:{}",
                    ctx.setup_id,
                    ctx.kind.as_db_str()
                ))
            } else {
                None
            };
            let res = sqlx::query(
                r#"INSERT INTO notify_outbox
                      (title, body, channels, severity, event_key,
                       exchange, segment, symbol, status, dedup_key)
                   VALUES ($1, $2, '["telegram"]'::jsonb, 'info', $3,
                           $4, 'futures', $5, 'pending', $6)
                   ON CONFLICT (dedup_key) WHERE dedup_key IS NOT NULL DO NOTHING"#,
            )
            .bind(&title)
            .bind(&body_html)
            .bind(format!("lifecycle.{}", ctx.kind.as_db_str()))
            .bind(&ctx.exchange)
            .bind(&ctx.symbol)
            .bind(dedup_key.as_deref())
            .execute(pool)
            .await;
            match res {
                Ok(r) if r.rows_affected() > 0 => debug!(
                    setup_id=%ctx.setup_id, kind=%ctx.kind.as_db_str(),
                    "TelegramLifecycleHandler: enqueued"
                ),
                Ok(_) => debug!(
                    setup_id=%ctx.setup_id, kind=%ctx.kind.as_db_str(),
                    "TelegramLifecycleHandler: dedup hit (already enqueued)"
                ),
                Err(e) => warn!(
                    %e, setup_id=%ctx.setup_id, kind=%ctx.kind.as_db_str(),
                    "TelegramLifecycleHandler: enqueue failed"
                ),
            }
            return;
        }

        // Fallback: direct send (test + legacy).
        if let Err(e) = self
            .dispatcher
            .send(NotificationChannel::Telegram, &n)
            .await
        {
            warn!(%e, setup_id=%ctx.setup_id, kind=%ctx.kind.as_db_str(),
                  "TelegramLifecycleHandler: send failed");
        } else {
            debug!(setup_id=%ctx.setup_id, kind=%ctx.kind.as_db_str(),
                   "TelegramLifecycleHandler: sent");
        }
    }
}

// ---------------------------------------------------------------------------
// X outbox handler (Faz 9.7.6)
// ---------------------------------------------------------------------------

use qtss_storage::{enqueue_x_outbox, XOutboxInsert};
use crate::x_render::render_lifecycle_x;

/// Enqueues the rendered X body into  for the publisher
/// loop to drain. Decoupling the renderer from the API call keeps
/// rate-limit and retry logic centralised in the publisher.
pub struct XOutboxHandler {
    pool: PgPool,
}

impl XOutboxHandler {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl LifecycleHandler for XOutboxHandler {
    fn name(&self) -> &'static str {
        "x_outbox"
    }

    async fn on_event(&self, ctx: &LifecycleContext) {
        let body = render_lifecycle_x(ctx);
        let ins = XOutboxInsert {
            setup_id: Some(ctx.setup_id),
            lifecycle_event_id: None,
            event_key: format!("lifecycle.{}", ctx.kind.as_db_str()),
            body,
            image_path: None,
        };
        if let Err(e) = enqueue_x_outbox(&self.pool, &ins).await {
            warn!(%e, setup_id=%ctx.setup_id, kind=%ctx.kind.as_db_str(),
                  "XOutboxHandler: enqueue failed");
        } else {
            debug!(setup_id=%ctx.setup_id, kind=%ctx.kind.as_db_str(),
                   "XOutboxHandler: enqueued");
        }
    }
}
