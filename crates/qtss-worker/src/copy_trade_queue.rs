//! Leader `exchange_orders` fill satırları → `copy_trade_execution_jobs` + tüketici (`docs/QTSS_CURSOR_DEV_GUIDE.md` §9.1 madde 4).
//! Follower dry `place` uses [`strategy_runner::wrap_shared_dry_gateway_for_persistence`] when auto-place is on (paper + same-table mirror as strategy runner).
//!
//! Tarama: `venue_response` içinde dolum ipucu olan son emirler (`list_recent_filled_orders_global_since`).
//! Aynı lider emri için abonelik başına en fazla bir iş (`UNIQUE (subscription_id, leader_exchange_order_id)`).

use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use qtss_common::is_trading_halted;
use qtss_domain::copy_trade::CopyRule;
use qtss_domain::orders::OrderIntent;
use qtss_execution::ExecutionGateway;
use qtss_storage::{
    CopySubscriptionRepository, CopySubscriptionRow, CopyTradeJobRepository, ExchangeOrderRepository,
    ExchangeOrderRow,
};
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::strategy_runner::{
    dry_gateway_from_pool, wrap_shared_dry_gateway_for_persistence, DryPersistenceKeys,
};

fn queue_enabled() -> bool {
    std::env::var("QTSS_COPY_TRADE_QUEUE_ENABLED")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn tick_secs() -> u64 {
    std::env::var("QTSS_COPY_TRADE_QUEUE_TICK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20)
        .max(5)
}

fn scan_limit() -> i64 {
    std::env::var("QTSS_COPY_TRADE_QUEUE_SCAN_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(400)
        .clamp(50, 2000)
}

fn scan_lookback_hours() -> i64 {
    std::env::var("QTSS_COPY_TRADE_QUEUE_SCAN_LOOKBACK_HOURS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(168)
        .clamp(1, 24 * 30)
}

fn auto_place() -> bool {
    std::env::var("QTSS_COPY_TRADE_QUEUE_AUTO_PLACE")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn requires_human_approval() -> bool {
    !std::env::var("QTSS_STRATEGY_SKIP_HUMAN_APPROVAL")
        .ok()
        .is_some_and(|s| matches!(s.trim(), "1" | "true" | "yes" | "on"))
}

fn parse_rule(v: &serde_json::Value) -> Option<CopyRule> {
    serde_json::from_value(v.clone()).ok()
}

fn symbol_allowed(rule: &CopyRule, sym: &str) -> bool {
    let u = sym.trim().to_uppercase();
    if u.is_empty() {
        return false;
    }
    if rule.symbol_allowlist.is_empty() {
        return true;
    }
    rule.symbol_allowlist
        .iter()
        .any(|s| s.trim().to_uppercase() == u)
}

async fn enqueue_from_fills(
    pool: &PgPool,
    subs: &[CopySubscriptionRow],
    orders: &[ExchangeOrderRow],
) -> u32 {
    let jobs = CopyTradeJobRepository::new(pool.clone());
    let mut enq = 0_u32;
    for o in orders {
        for sub in subs.iter().filter(|s| s.leader_user_id == o.user_id) {
            let Some(rule) = parse_rule(&sub.rule) else {
                warn!(sub_id = %sub.id, "copy_trade_queue: CopyRule parse");
                continue;
            };
            if !symbol_allowed(&rule, &o.symbol) {
                continue;
            }
            let Ok(leader_intent) = serde_json::from_value::<OrderIntent>(o.intent.clone()) else {
                warn!(order_id = %o.id, "copy_trade_queue: OrderIntent parse");
                continue;
            };
            let qty = leader_intent.quantity * rule.size_multiplier;
            if qty <= Decimal::ZERO {
                continue;
            }
            let mut follower_intent = leader_intent.clone();
            follower_intent.quantity = qty;
            follower_intent.requires_human_approval = requires_human_approval();
            let payload = json!({ "intent": follower_intent });
            match jobs
                .try_enqueue(
                    sub.id,
                    o.id,
                    sub.follower_user_id,
                    sub.leader_user_id,
                    payload,
                )
                .await
            {
                Ok(true) => {
                    enq += 1;
                    info!(
                        sub_id = %sub.id,
                        leader_order = %o.id,
                        follower = %sub.follower_user_id,
                        "copy_trade_queue: job enqueued"
                    );
                }
                Ok(false) => {}
                Err(e) => warn!(%e, "copy_trade_queue: try_enqueue"),
            }
        }
    }
    enq
}

const MAX_JOBS_PER_TICK: usize = 50;

async fn process_jobs(pool: &PgPool, gw: Option<&Arc<dyn ExecutionGateway>>) {
    let jobs = CopyTradeJobRepository::new(pool.clone());
    let auto = auto_place();
    for _ in 0..MAX_JOBS_PER_TICK {
        let job = match jobs.claim_next_pending().await {
            Ok(j) => j,
            Err(e) => {
                warn!(%e, "copy_trade_queue: claim_next_pending");
                break;
            }
        };
        let Some(job) = job else {
            break;
        };
        let Some(intent_val) = job.payload.get("intent") else {
            if let Err(e) = jobs
                .mark_failed(job.id, "payload missing intent field")
                .await
            {
                warn!(%e, "copy_trade_queue: mark_failed");
            }
            continue;
        };
        let intent: OrderIntent = match serde_json::from_value(intent_val.clone()) {
            Ok(i) => i,
            Err(e) => {
                let msg = format!("intent json: {e}");
                if let Err(err) = jobs.mark_failed(job.id, &msg).await {
                    warn!(%err, "copy_trade_queue: mark_failed");
                }
                continue;
            }
        };
        if !auto {
            if let Err(e) = jobs
                .mark_skipped(job.id, "QTSS_COPY_TRADE_QUEUE_AUTO_PLACE off")
                .await
            {
                warn!(%e, "copy_trade_queue: mark_skipped");
            }
            continue;
        }
        let Some(g) = gw else {
            if let Err(e) = jobs.mark_skipped(job.id, "no execution gateway").await {
                warn!(%e, "copy_trade_queue: mark_skipped");
            }
            continue;
        };
        if is_trading_halted() {
            if let Err(e) = jobs.mark_skipped(job.id, "trading halted (kill switch)").await {
                warn!(%e, "copy_trade_queue: mark_skipped");
            }
            continue;
        }
        match g.place(intent).await {
            Ok(id) => {
                info!(job_id = %job.id, ?id, "copy_trade_queue: follower dry place");
                if let Err(e) = jobs.mark_done(job.id).await {
                    warn!(%e, "copy_trade_queue: mark_done");
                }
            }
            Err(e) => {
                let msg = e.to_string();
                warn!(job_id = %job.id, %msg, "copy_trade_queue: place failed");
                if let Err(err) = jobs.mark_failed(job.id, &msg).await {
                    warn!(%err, "copy_trade_queue: mark_failed");
                }
            }
        }
    }
}

pub async fn copy_trade_queue_loop(pool: PgPool) {
    if !queue_enabled() {
        info!("QTSS_COPY_TRADE_QUEUE_ENABLED off — copy_trade_queue_loop exit");
        return;
    }
    let tick = Duration::from_secs(tick_secs());
    let subs_repo = CopySubscriptionRepository::new(pool.clone());
    let ord_repo = ExchangeOrderRepository::new(pool.clone());
    let gw: Option<Arc<dyn ExecutionGateway>> = if auto_place() {
        let dry = dry_gateway_from_pool(&pool).await;
        Some(
            wrap_shared_dry_gateway_for_persistence(
                dry,
                &pool,
                DryPersistenceKeys::uniform("copy_trade_queue"),
            )
            .await,
        )
    } else {
        None
    };
    info!(
        poll_secs = tick.as_secs(),
        scan_limit = scan_limit(),
        lookback_h = scan_lookback_hours(),
        auto_place = auto_place(),
        "copy_trade_queue_loop (leader fills → jobs → optional dry place)"
    );
    loop {
        tokio::time::sleep(tick).await;
        let subs = match subs_repo.list_active_subscriptions().await {
            Ok(s) => s,
            Err(e) => {
                warn!(%e, "copy_trade_queue: list_active_subscriptions");
                continue;
            }
        };
        if subs.is_empty() {
            tracing::debug!("copy_trade_queue: no active subscriptions");
            continue;
        }
        let since = Utc::now() - ChronoDuration::hours(scan_lookback_hours());
        let filled = match ord_repo
            .list_recent_filled_orders_global_since(since, scan_limit())
            .await
        {
            Ok(f) => f,
            Err(e) => {
                warn!(%e, "copy_trade_queue: list_recent_filled_orders_global_since");
                continue;
            }
        };
        let n = enqueue_from_fills(&pool, &subs, &filled).await;
        if n > 0 {
            info!(enqueued = n, "copy_trade_queue: enqueue batch");
        }
        process_jobs(&pool, gw.as_ref()).await;
    }
}
