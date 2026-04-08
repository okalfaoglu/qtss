//! Paper-fill simulator adapter.
//!
//! Backs `dry` and `backtest` modes. Fills:
//! - Market orders immediately at the *reference price* + slippage
//! - Limit orders at their stated price (assumes price touches)
//! - Stop orders at their stop_price + slippage
//!
//! Reference price is supplied by the caller via [`SimAdapter::set_reference_price`]
//! before placing the order. Slippage is configured at construction time
//! and pulled from `qtss_config` (CLAUDE.md rule #2 — no hardcoded magic).

use crate::adapter::{ExecutionAdapter, Fill, OrderAck, OrderStatus};
use crate::error::{ExecutionError, ExecutionResult};
use async_trait::async_trait;
use chrono::Utc;
use qtss_domain::v2::intent::{OrderRequest, OrderType, Side};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SimConfig {
    /// Slippage as a fraction of price (e.g. 0.0005 = 5 bps).
    pub slippage_pct: Decimal,
    /// Taker fee as a fraction of notional.
    pub taker_fee_pct: Decimal,
}

impl SimConfig {
    pub fn defaults() -> Self {
        Self {
            slippage_pct: dec!(0.0005),
            taker_fee_pct: dec!(0.0007),
        }
    }
}

pub struct SimAdapter {
    config: SimConfig,
    reference_price: Mutex<Option<Decimal>>,
    orders: Mutex<HashMap<Uuid, OrderAck>>,
}

impl SimAdapter {
    pub fn new(config: SimConfig) -> Self {
        Self {
            config,
            reference_price: Mutex::new(None),
            orders: Mutex::new(HashMap::new()),
        }
    }

    pub fn set_reference_price(&self, price: Decimal) {
        *self.reference_price.lock().unwrap() = Some(price);
    }

    pub fn snapshot(&self) -> Vec<OrderAck> {
        self.orders.lock().unwrap().values().cloned().collect()
    }

    fn fill_price(&self, req: &OrderRequest) -> ExecutionResult<Decimal> {
        let slip = self.config.slippage_pct;
        match req.order_type {
            OrderType::Market => {
                let r = self
                    .reference_price
                    .lock()
                    .unwrap()
                    .ok_or_else(|| ExecutionError::Adapter("no reference price set".into()))?;
                Ok(apply_slip(r, req.side, slip))
            }
            OrderType::Limit => req
                .price
                .ok_or_else(|| ExecutionError::InvalidIntent("limit order needs price".into())),
            OrderType::Stop => {
                let sp = req
                    .stop_price
                    .ok_or_else(|| ExecutionError::InvalidIntent("stop order needs stop_price".into()))?;
                Ok(apply_slip(sp, req.side, slip))
            }
            OrderType::StopLimit => req
                .price
                .ok_or_else(|| ExecutionError::InvalidIntent("stop_limit needs price".into())),
            OrderType::Oco | OrderType::Iceberg => Err(ExecutionError::InvalidIntent(
                "oco/iceberg not supported by sim".into(),
            )),
        }
    }
}

fn apply_slip(price: Decimal, side: Side, slip: Decimal) -> Decimal {
    let delta = price * slip;
    match side {
        Side::Long => price + delta,   // pay up
        Side::Short => price - delta,  // hit the bid
    }
}

#[async_trait]
impl ExecutionAdapter for SimAdapter {
    fn name(&self) -> &'static str {
        "sim"
    }

    async fn place(&self, req: OrderRequest) -> ExecutionResult<OrderAck> {
        let price = self.fill_price(&req)?;
        let notional = price * req.quantity;
        let fee = notional * self.config.taker_fee_pct;
        let now = Utc::now();
        let ack = OrderAck {
            client_order_id: req.client_order_id,
            venue_order_id: format!("sim-{}", req.client_order_id),
            status: OrderStatus::Filled,
            accepted_at: now,
            fills: vec![Fill {
                fill_id: Uuid::new_v4(),
                order_id: req.client_order_id,
                price,
                quantity: req.quantity,
                fee,
                at: now,
            }],
        };
        self.orders
            .lock()
            .unwrap()
            .insert(req.client_order_id, ack.clone());
        Ok(ack)
    }

    async fn cancel(&self, client_order_id: Uuid) -> ExecutionResult<()> {
        let mut orders = self.orders.lock().unwrap();
        let ack = orders
            .get_mut(&client_order_id)
            .ok_or(ExecutionError::OrderNotFound(client_order_id))?;
        if matches!(ack.status, OrderStatus::Filled) {
            return Err(ExecutionError::Adapter("already filled".into()));
        }
        ack.status = OrderStatus::Canceled;
        Ok(())
    }

    async fn status(&self, client_order_id: Uuid) -> ExecutionResult<OrderAck> {
        self.orders
            .lock()
            .unwrap()
            .get(&client_order_id)
            .cloned()
            .ok_or(ExecutionError::OrderNotFound(client_order_id))
    }
}
