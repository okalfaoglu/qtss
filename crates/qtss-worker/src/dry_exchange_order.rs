//! Dry (simulated) fills persisted like live rows in `exchange_orders` (`status = filled`, no venue HTTP).
//!
//! Resolution order for `org_id` / `user_id` (same actor used for live tactical executor when no book):
//! 1. Book owner (`position_manager`: org + user from aggregated position)
//! 2. Book user only → match `filled_hint` row for same symbol to recover `org_id`
//! 3. `QTSS_AI_TACTICAL_EXECUTOR_ORG_ID` + `QTSS_AI_TACTICAL_EXECUTOR_USER_ID`
//! 4. `worker.paper_org_id` / `worker.paper_user_id` (or `QTSS_PAPER_ORG_ID` / `QTSS_PAPER_USER_ID`)
//! 5. Any recent filled `exchange_orders` row for the symbol

use qtss_domain::orders::OrderIntent;
use qtss_execution::{DryPlaceOutcome, DryRunGateway, ExecutionError, ExecutionGateway};
use qtss_storage::{
    resolve_system_string, ExchangeOrderRepository, ExchangeOrderRow,
};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

#[must_use]
pub fn env_ai_tactical_org_user() -> Option<(Uuid, Uuid)> {
    let org = std::env::var("QTSS_AI_TACTICAL_EXECUTOR_ORG_ID")
        .ok()
        .and_then(|s| Uuid::parse_str(s.trim()).ok());
    let user = std::env::var("QTSS_AI_TACTICAL_EXECUTOR_USER_ID")
        .ok()
        .and_then(|s| Uuid::parse_str(s.trim()).ok());
    match (org, user) {
        (Some(o), Some(u)) => Some((o, u)),
        _ => None,
    }
}

pub async fn resolve_dry_exchange_order_party(
    pool: &PgPool,
    org_from_book: Option<Uuid>,
    user_from_book: Option<Uuid>,
    filled_hint: &[ExchangeOrderRow],
    symbol: &str,
) -> Option<(Uuid, Uuid)> {
    let sym = symbol.trim();
    if let (Some(o), Some(u)) = (org_from_book, user_from_book) {
        return Some((o, u));
    }
    if let Some(u) = user_from_book {
        if let Some(r) = filled_hint.iter().find(|r| {
            r.user_id == u && r.symbol.trim().eq_ignore_ascii_case(sym)
        }) {
            return Some((r.org_id, u));
        }
    }
    if let Some(p) = env_ai_tactical_org_user() {
        return Some(p);
    }
    let org_s = resolve_system_string(
        pool,
        "worker",
        "paper_org_id",
        "QTSS_PAPER_ORG_ID",
        "",
    )
    .await;
    let user_s = resolve_system_string(
        pool,
        "worker",
        "paper_user_id",
        "QTSS_PAPER_USER_ID",
        "",
    )
    .await;
    if let (Ok(o), Ok(u)) = (
        Uuid::parse_str(org_s.trim()),
        Uuid::parse_str(user_s.trim()),
    ) {
        return Some((o, u));
    }
    filled_hint
        .iter()
        .find(|r| r.symbol.trim().eq_ignore_ascii_case(sym))
        .map(|r| (r.org_id, r.user_id))
}

#[must_use]
pub fn merge_dry_venue_response(
    simulation_source: &str,
    outcome: &DryPlaceOutcome,
    extra: Value,
) -> Value {
    let mut v = json!({
        "dry_run": true,
        "simulation_source": simulation_source,
        "status": "FILLED",
        "executedQty": outcome.fill.quantity.to_string(),
        "avgPrice": outcome.fill.avg_price.to_string(),
        "fee": outcome.fill.fee.to_string(),
        "note": "Simulated fill; no HTTP request sent to the exchange.",
    });
    if let Some(obj) = v.as_object_mut() {
        if let Value::Object(m) = extra {
            for (k, val) in m {
                obj.insert(k.clone(), val);
            }
        }
    }
    v
}

pub async fn insert_dry_fill_exchange_orders(
    repo: &ExchangeOrderRepository,
    org_id: Uuid,
    user_id: Uuid,
    exchange_slug: &str,
    segment_db: &str,
    symbol: &str,
    intent: &OrderIntent,
    outcome: &DryPlaceOutcome,
    simulation_source: &str,
    extra: Value,
) -> Result<ExchangeOrderRow, qtss_storage::StorageError> {
    let venue = merge_dry_venue_response(simulation_source, outcome, extra);
    repo.insert_dry_simulated_filled(
        org_id,
        user_id,
        exchange_slug.trim(),
        segment_db.trim(),
        symbol.trim(),
        outcome.client_order_id,
        intent,
        venue,
    )
    .await
}

/// Reference price + simulated market fill + best-effort `exchange_orders` row (same shape as live fills).
pub async fn persist_after_dry_place(
    gw: &DryRunGateway,
    pool: &PgPool,
    repo: &ExchangeOrderRepository,
    intent: &OrderIntent,
    mark: Decimal,
    symbol: &str,
    exchange_slug: &str,
    segment_db: &str,
    simulation_source: &str,
    extra: Value,
    filled_hint: &[ExchangeOrderRow],
    org_from_book: Option<Uuid>,
    user_from_book: Option<Uuid>,
) -> Result<DryPlaceOutcome, ExecutionError> {
    gw.set_reference_price(&intent.instrument, mark)?;
    let out = gw.place_detailed(intent.clone(), None)?;
    let party = resolve_dry_exchange_order_party(
        pool,
        org_from_book,
        user_from_book,
        filled_hint,
        symbol,
    )
    .await;
    let Some((org_id, user_id)) = party else {
        warn!(
            %symbol,
            cid = %out.client_order_id,
            %simulation_source,
            "dry_exchange_order: no org/user for exchange_orders — set worker.paper_org_id/paper_user_id (UUIDs) and/or QTSS_AI_TACTICAL_EXECUTOR_ORG_ID/USER_ID"
        );
        return Ok(out);
    };
    match insert_dry_fill_exchange_orders(
        repo,
        org_id,
        user_id,
        exchange_slug,
        segment_db,
        symbol,
        intent,
        &out,
        simulation_source,
        extra,
    )
    .await
    {
        Ok(row) => {
            info!(
                %symbol,
                cid = %out.client_order_id,
                exchange_order_row_id = %row.id,
                %simulation_source,
                "dry_exchange_order: simulated fill persisted to exchange_orders"
            );
        }
        Err(e) => {
            warn!(
                %e,
                %symbol,
                cid = %out.client_order_id,
                %simulation_source,
                "dry_exchange_order: insert_dry_simulated_filled failed"
            );
        }
    }
    Ok(out)
}
