//! Execution adapter trait + types.
//!
//! Every venue / mode (live spot, live futures, paper sim, backtest)
//! implements this same trait. The router picks an adapter by
//! `ExecutionMode` so the call sites stay venue-agnostic.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use qtss_domain::v2::intent::OrderRequest;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ExecutionResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    pub fill_id: Uuid,
    pub order_id: Uuid,
    pub price: Decimal,
    pub quantity: Decimal,
    pub fee: Decimal,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderAck {
    pub client_order_id: Uuid,
    pub venue_order_id: String,
    pub status: OrderStatus,
    pub accepted_at: DateTime<Utc>,
    pub fills: Vec<Fill>,
}

/// Identifies an outstanding order across crates. Most venues
/// require both the symbol *and* the client/venue id to look an
/// order up — this struct keeps that pair as one value so adapter
/// signatures stay symmetric.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrderHandle {
    pub symbol: String,
    pub client_order_id: Uuid,
}

impl OrderHandle {
    pub fn new(symbol: impl Into<String>, client_order_id: Uuid) -> Self {
        Self { symbol: symbol.into(), client_order_id }
    }
}

#[async_trait]
pub trait ExecutionAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    async fn place(&self, req: OrderRequest) -> ExecutionResult<OrderAck>;
    async fn cancel(&self, handle: &OrderHandle) -> ExecutionResult<()>;
    async fn status(&self, handle: &OrderHandle) -> ExecutionResult<OrderAck>;
}
