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

/// Book + hint + env-style inputs without hitting the database (for tests and [`resolve_dry_exchange_order_party`]).
#[must_use]
pub(crate) fn resolve_dry_exchange_order_party_sync(
    org_from_book: Option<Uuid>,
    user_from_book: Option<Uuid>,
    filled_hint: &[ExchangeOrderRow],
    symbol: &str,
    ai_tactical_org_user: Option<(Uuid, Uuid)>,
    paper_org_user: Option<(Uuid, Uuid)>,
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
    if let Some(p) = ai_tactical_org_user {
        return Some(p);
    }
    if let Some(p) = paper_org_user {
        return Some(p);
    }
    filled_hint
        .iter()
        .find(|r| r.symbol.trim().eq_ignore_ascii_case(sym))
        .map(|r| (r.org_id, r.user_id))
}

pub async fn resolve_dry_exchange_order_party(
    pool: &PgPool,
    org_from_book: Option<Uuid>,
    user_from_book: Option<Uuid>,
    filled_hint: &[ExchangeOrderRow],
    symbol: &str,
) -> Option<(Uuid, Uuid)> {
    let ai = env_ai_tactical_org_user();
    if let Some(party) = resolve_dry_exchange_order_party_sync(
        org_from_book,
        user_from_book,
        filled_hint,
        symbol,
        ai,
        None,
    ) {
        return Some(party);
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
    let paper_org_user = match (
        Uuid::parse_str(org_s.trim()),
        Uuid::parse_str(user_s.trim()),
    ) {
        (Ok(o), Ok(u)) => Some((o, u)),
        _ => None,
    };
    resolve_dry_exchange_order_party_sync(
        org_from_book,
        user_from_book,
        filled_hint,
        symbol,
        None,
        paper_org_user,
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use qtss_execution::FillEvent;
    use rust_decimal::Decimal;
    use std::collections::HashMap;

    fn hint_row(org_id: Uuid, user_id: Uuid, symbol: &str) -> ExchangeOrderRow {
        ExchangeOrderRow {
            id: Uuid::from_u128(0xa100),
            org_id,
            user_id,
            exchange: "binance".into(),
            segment: "futures".into(),
            symbol: symbol.into(),
            client_order_id: Uuid::from_u128(0xb100),
            status: "filled".into(),
            intent: json!({}),
            venue_order_id: None,
            venue_response: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn sample_outcome() -> DryPlaceOutcome {
        let cid = Uuid::from_u128(1u128);
        DryPlaceOutcome {
            client_order_id: cid,
            fill: FillEvent {
                client_order_id: cid,
                avg_price: Decimal::new(42_000, 0),
                quantity: Decimal::new(1, 3),
                fee: Decimal::new(5, 2),
            },
            quote_balance_after: Decimal::ZERO,
            base_positions_after: HashMap::new(),
        }
    }

    #[test]
    fn merge_dry_venue_response_merges_extra_into_base() {
        let out = sample_outcome();
        let v = merge_dry_venue_response("unit_test", &out, json!({ "trace": "x" }));
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["simulation_source"], "unit_test");
        assert_eq!(v["status"], "FILLED");
        assert_eq!(v["executedQty"], "0.001");
        assert_eq!(v["avgPrice"], "42000");
        assert_eq!(v["trace"], "x");
    }

    #[test]
    fn resolve_sync_prefers_full_book_party() {
        let book_o = Uuid::from_u128(0x101);
        let book_u = Uuid::from_u128(0x102);
        let hint_o = Uuid::from_u128(0x201);
        let hint_u = Uuid::from_u128(0x202);
        let hints = [hint_row(hint_o, hint_u, "BTCUSDT")];
        let ai = Uuid::from_u128(0x301);
        let au = Uuid::from_u128(0x302);
        let out = resolve_dry_exchange_order_party_sync(
            Some(book_o),
            Some(book_u),
            &hints,
            "BTCUSDT",
            Some((ai, au)),
            Some((Uuid::from_u128(0x401), Uuid::from_u128(0x402))),
        );
        assert_eq!(out, Some((book_o, book_u)));
    }

    #[test]
    fn resolve_sync_book_user_only_matches_hint_symbol() {
        let org = Uuid::from_u128(0x501);
        let user = Uuid::from_u128(0x502);
        let hints = [hint_row(org, user, "btcusdt")];
        let out = resolve_dry_exchange_order_party_sync(
            None,
            Some(user),
            &hints,
            "BTCUSDT",
            None,
            None,
        );
        assert_eq!(out, Some((org, user)));
    }

    #[test]
    fn resolve_sync_uses_ai_when_book_and_hint_do_not_resolve() {
        let ai_o = Uuid::from_u128(0x601);
        let ai_u = Uuid::from_u128(0x602);
        let hints: [ExchangeOrderRow; 0] = [];
        let out = resolve_dry_exchange_order_party_sync(
            None,
            None,
            &hints,
            "ETHUSDT",
            Some((ai_o, ai_u)),
            None,
        );
        assert_eq!(out, Some((ai_o, ai_u)));
    }

    #[test]
    fn resolve_sync_uses_paper_when_ai_absent() {
        let p_o = Uuid::from_u128(0x701);
        let p_u = Uuid::from_u128(0x702);
        let hints: [ExchangeOrderRow; 0] = [];
        let out = resolve_dry_exchange_order_party_sync(
            None,
            None,
            &hints,
            "SOLUSDT",
            None,
            Some((p_o, p_u)),
        );
        assert_eq!(out, Some((p_o, p_u)));
    }

    #[test]
    fn resolve_sync_falls_back_to_any_hint_row_for_symbol() {
        let org = Uuid::from_u128(0x801);
        let user = Uuid::from_u128(0x802);
        let hints = [hint_row(org, user, "  xrpusdt  ")];
        let out = resolve_dry_exchange_order_party_sync(
            None,
            None,
            &hints,
            "XRPUSDT",
            None,
            None,
        );
        assert_eq!(out, Some((org, user)));
    }

    #[test]
    fn resolve_sync_when_book_user_has_no_matching_row_uses_symbol_fallback() {
        let org = Uuid::from_u128(0x901);
        let row_user = Uuid::from_u128(0x902);
        let other_user = Uuid::from_u128(0x903);
        let hints = [hint_row(org, row_user, "ADAUSDT")];
        let out = resolve_dry_exchange_order_party_sync(
            None,
            Some(other_user),
            &hints,
            "ADAUSDT",
            None,
            None,
        );
        assert_eq!(out, Some((org, row_user)));
    }
}
