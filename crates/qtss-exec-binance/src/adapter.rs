//! Binance spot `ExecutionAdapter` implementation.
//!
//! Wraps a [`qtss_binance::BinanceClient`] and a [`FeeModel`]. The
//! adapter is intentionally thin: it converts the v2 [`OrderRequest`]
//! to Binance fields, posts the order, and lifts the JSON response
//! into a generic [`OrderAck`]. Cancel and status calls go through
//! the same client.
//!
//! Cancel/status correlate by the v2 `client_order_id` (UUID), which
//! we hand to Binance as `newClientOrderId`. Binance accepts up to 36
//! chars and a UUID is 36, so the round-trip is lossless.

use crate::error::BinanceExecError;
use crate::translate::{build_spot_payload, SpotPayload};
use async_trait::async_trait;
use chrono::Utc;
use qtss_binance::{
    venue_order_id_from_binance_order_response, BinanceClient,
};
use qtss_domain::v2::intent::OrderRequest;
use qtss_execution_v2::{
    ExecutionAdapter, ExecutionError, ExecutionResult, Fill, OrderAck, OrderHandle, OrderStatus,
};
use qtss_fees::{FeeModel, Liquidity, TradeContext};
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

/// Per-environment knobs. All values are pulled from `qtss_config` by
/// the bootstrap layer (CLAUDE.md rule #2). Nothing here is defaulted.
#[derive(Debug, Clone)]
pub struct BinanceExecConfig {
    /// `newOrderRespType` to request — typically `"FULL"` so we get
    /// the executed fills back in the same response, or `"ACK"` for
    /// reduced latency.
    pub new_order_resp_type: String,
}

pub struct BinanceExecAdapter {
    client: Arc<BinanceClient>,
    fees: Arc<dyn FeeModel>,
    config: BinanceExecConfig,
}

impl BinanceExecAdapter {
    pub fn new(
        client: Arc<BinanceClient>,
        fees: Arc<dyn FeeModel>,
        config: BinanceExecConfig,
    ) -> Self {
        Self { client, fees, config }
    }

    fn synth_fee(&self, req: &OrderRequest, price: Decimal) -> ExecutionResult<Decimal> {
        let liq = if req.post_only {
            Liquidity::Maker
        } else {
            Liquidity::Taker
        };
        self.fees
            .quote(&TradeContext {
                venue: req.instrument.venue.as_key(),
                symbol: &req.instrument.symbol,
                price,
                quantity: req.quantity,
                liquidity: liq,
            })
            .map(|q| q.total)
            .map_err(|e| {
                ExecutionError::Adapter(BinanceExecError::Fees(e.to_string()).to_string())
            })
    }
}

fn map_status(s: Option<&str>) -> OrderStatus {
    match s.unwrap_or("") {
        "FILLED" => OrderStatus::Filled,
        "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
        "CANCELED" | "EXPIRED" => OrderStatus::Canceled,
        "REJECTED" => OrderStatus::Rejected,
        _ => OrderStatus::New,
    }
}

fn parse_decimal(v: Option<&serde_json::Value>) -> Option<Decimal> {
    v.and_then(|x| x.as_str())
        .and_then(|s| Decimal::from_str(s).ok())
}

#[async_trait]
impl ExecutionAdapter for BinanceExecAdapter {
    fn name(&self) -> &'static str {
        "binance-spot"
    }

    async fn place(&self, req: OrderRequest) -> ExecutionResult<OrderAck> {
        let payload: SpotPayload = build_spot_payload(&req)
            .map_err(|e| ExecutionError::Adapter(e.to_string()))?;

        let resp = self
            .client
            .spot_new_order(
                &req.instrument.symbol,
                payload.side,
                payload.bn_type,
                payload.tif,
                Some(payload.quantity.as_str()),
                None,
                payload.price.as_deref(),
                Some(payload.client_order_id.as_str()),
                payload.stop_price.as_deref(),
                None,
                Some(self.config.new_order_resp_type.as_str()),
            )
            .await
            .map_err(|e| ExecutionError::Adapter(format!("binance: {e}")))?;

        let venue_order_id = venue_order_id_from_binance_order_response(&resp)
            .map(|i| i.to_string())
            .ok_or_else(|| {
                ExecutionError::Adapter(
                    BinanceExecError::MalformedResponse("missing orderId".into()).to_string(),
                )
            })?;

        let status = map_status(resp.get("status").and_then(|v| v.as_str()));
        let now = Utc::now();

        // Lift the FULL response fills array. If the venue returned
        // ACK we synthesise a single fill at the request price so the
        // portfolio engine still gets a deterministic fee.
        let mut fills = Vec::new();
        if let Some(arr) = resp.get("fills").and_then(|v| v.as_array()) {
            for f in arr {
                let price = parse_decimal(f.get("price")).unwrap_or_default();
                let qty = parse_decimal(f.get("qty")).unwrap_or_default();
                let commission = parse_decimal(f.get("commission")).unwrap_or_default();
                fills.push(Fill {
                    fill_id: Uuid::new_v4(),
                    order_id: req.client_order_id,
                    price,
                    quantity: qty,
                    fee: commission,
                    at: now,
                });
            }
        }
        if fills.is_empty() && matches!(status, OrderStatus::Filled) {
            let price = req.price.unwrap_or_default();
            let fee = self.synth_fee(&req, price)?;
            fills.push(Fill {
                fill_id: Uuid::new_v4(),
                order_id: req.client_order_id,
                price,
                quantity: req.quantity,
                fee,
                at: now,
            });
        }

        Ok(OrderAck {
            client_order_id: req.client_order_id,
            venue_order_id,
            status,
            accepted_at: now,
            fills,
        })
    }

    async fn cancel(&self, handle: &OrderHandle) -> ExecutionResult<()> {
        let coid = handle.client_order_id.to_string();
        self.client
            .spot_cancel_order(&handle.symbol, None, Some(&coid), None)
            .await
            .map(|_| ())
            .map_err(|e| ExecutionError::Adapter(format!("binance cancel: {e}")))
    }

    async fn status(&self, handle: &OrderHandle) -> ExecutionResult<OrderAck> {
        let coid = handle.client_order_id.to_string();
        let resp = self
            .client
            .spot_query_order(&handle.symbol, None, Some(&coid))
            .await
            .map_err(|e| ExecutionError::Adapter(format!("binance status: {e}")))?;
        let venue_order_id = venue_order_id_from_binance_order_response(&resp)
            .map(|i| i.to_string())
            .unwrap_or_default();
        let status = map_status(resp.get("status").and_then(|v| v.as_str()));
        Ok(OrderAck {
            client_order_id: handle.client_order_id,
            venue_order_id,
            status,
            accepted_at: Utc::now(),
            fills: vec![],
        })
    }
}
