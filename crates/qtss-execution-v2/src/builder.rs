//! ApprovedIntent → OrderRequest splitter.
//!
//! An ApprovedIntent is a single high-level "open this position with
//! these exits" instruction. We translate it into a small bracket of
//! venue-agnostic OrderRequests:
//!
//!   1. Parent entry order  (Limit if entry_price set, else Market)
//!   2. Stop-loss exit      (Stop, reduce_only)
//!   3. One Limit per take-profit target (reduce_only, child quantity =
//!      parent_qty * target.weight, normalised so children sum to parent)
//!
//! This stays pure data — actual placement happens in the adapter.

use crate::error::{ExecutionError, ExecutionResult};
use qtss_domain::v2::detection::Target;
use qtss_domain::v2::intent::{
    ApprovedIntent, OrderRequest, OrderType, Side, TimeInForce, TradeIntent,
};
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct OrderBracket {
    pub entry: OrderRequest,
    pub stop: OrderRequest,
    pub take_profits: Vec<OrderRequest>,
}

impl OrderBracket {
    pub fn iter(&self) -> impl Iterator<Item = &OrderRequest> {
        std::iter::once(&self.entry)
            .chain(std::iter::once(&self.stop))
            .chain(self.take_profits.iter())
    }
}

pub fn split_intent(approved: &ApprovedIntent) -> ExecutionResult<OrderBracket> {
    let intent = &approved.intent;
    if approved.quantity <= Decimal::ZERO {
        return Err(ExecutionError::InvalidIntent(
            "quantity must be > 0".into(),
        ));
    }
    let entry = build_entry(intent, approved.quantity, Some(approved.id));
    let stop = build_stop(intent, approved.quantity, Some(approved.id));
    let take_profits = build_take_profits(intent, approved.quantity, Some(approved.id));
    Ok(OrderBracket {
        entry,
        stop,
        take_profits,
    })
}

fn build_entry(intent: &TradeIntent, qty: Decimal, intent_id: Option<Uuid>) -> OrderRequest {
    let order_type = if intent.entry_price.is_some() {
        OrderType::Limit
    } else {
        OrderType::Market
    };
    OrderRequest {
        client_order_id: Uuid::new_v4(),
        instrument: intent.instrument.clone(),
        side: intent.side,
        order_type,
        quantity: qty,
        price: intent.entry_price,
        stop_price: None,
        time_in_force: intent.time_in_force,
        reduce_only: false,
        post_only: false,
        intent_id,
    }
}

fn build_stop(intent: &TradeIntent, qty: Decimal, intent_id: Option<Uuid>) -> OrderRequest {
    OrderRequest {
        client_order_id: Uuid::new_v4(),
        instrument: intent.instrument.clone(),
        side: opposite(intent.side),
        order_type: OrderType::Stop,
        quantity: qty,
        price: None,
        stop_price: Some(intent.stop_loss),
        time_in_force: TimeInForce::Gtc,
        reduce_only: true,
        post_only: false,
        intent_id,
    }
}

fn build_take_profits(
    intent: &TradeIntent,
    qty: Decimal,
    intent_id: Option<Uuid>,
) -> Vec<OrderRequest> {
    if intent.take_profits.is_empty() {
        return Vec::new();
    }
    let weights: Vec<Decimal> = intent
        .take_profits
        .iter()
        .map(|t| Decimal::from_f32_retain(t.weight).unwrap_or(Decimal::ZERO).max(Decimal::ZERO))
        .collect();
    let total: Decimal = weights.iter().sum();
    let normalised: Vec<Decimal> = if total > Decimal::ZERO {
        weights.iter().map(|w| *w / total).collect()
    } else {
        // Fallback: equal split.
        let n = Decimal::from(intent.take_profits.len() as i64);
        weights.iter().map(|_| Decimal::ONE / n).collect()
    };
    let mut allocated = Decimal::ZERO;
    let mut out: Vec<OrderRequest> = Vec::with_capacity(intent.take_profits.len());
    for (i, (target, frac)) in intent.take_profits.iter().zip(normalised.iter()).enumerate() {
        // Last target absorbs the dust to ensure children sum to parent.
        let child_qty = if i + 1 == intent.take_profits.len() {
            qty - allocated
        } else {
            let q = qty * *frac;
            allocated += q;
            q
        };
        out.push(build_tp(intent, target, child_qty, intent_id));
    }
    out
}

fn build_tp(intent: &TradeIntent, target: &Target, qty: Decimal, intent_id: Option<Uuid>) -> OrderRequest {
    OrderRequest {
        client_order_id: Uuid::new_v4(),
        instrument: intent.instrument.clone(),
        side: opposite(intent.side),
        order_type: OrderType::Limit,
        quantity: qty,
        price: Some(target.price),
        stop_price: None,
        time_in_force: TimeInForce::Gtc,
        reduce_only: true,
        post_only: false,
        intent_id,
    }
}

fn opposite(s: Side) -> Side {
    match s {
        Side::Long => Side::Short,
        Side::Short => Side::Long,
    }
}
