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

/// Renders each lifecycle event with [] and ships it
/// to Telegram through the shared []. A missing
/// Telegram configuration is treated as a no-op (the dispatcher returns
/// ); we log at warn and move on so the rest of
/// the router keeps working.
pub struct TelegramLifecycleHandler {
    dispatcher: NotificationDispatcher,
}

impl TelegramLifecycleHandler {
    pub fn new(dispatcher: NotificationDispatcher) -> Self {
        Self { dispatcher }
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
