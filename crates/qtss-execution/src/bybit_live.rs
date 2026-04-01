//! Bybit v5 unified **`ExecutionGateway`** — USDT **linear** (`category = linear`) **Market** / **Limit** + cancel by `orderLinkId` (`QTSS_MASTER_DEV_GUIDE` §2.3.12).
//! Spot, stop, trailing: not wired; use Binance or extend here.

use async_trait::async_trait;
use chrono::Utc;
use hmac::{Hmac, Mac};
use qtss_common::{log_business, Loggable, QtssLogLevel};
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{OrderIntent, OrderSide, OrderType, TimeInForce};
use qtss_domain::symbol::InstrumentId;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sha2::Sha256;
use tracing::instrument;
use uuid::Uuid;

use crate::gateway::{ExecutionError, ExecutionGateway};

type HmacSha256 = Hmac<Sha256>;

/// Parses `POST /v5/order` JSON body: top-level or nested `result.orderId` (string or number).
#[must_use]
pub fn venue_order_id_from_bybit_v5_response(v: &Value) -> Option<i64> {
    let obj = v.get("result").unwrap_or(v);
    obj.get("orderId")
        .and_then(|x| x.as_i64())
        .or_else(|| obj.get("orderId").and_then(|x| x.as_u64()).map(|u| u as i64))
        .or_else(|| {
            obj.get("orderId")
                .and_then(|x| x.as_str())
                .and_then(|s| s.parse().ok())
        })
}

/// Live USDT-M Bybit gateway (mainnet `https://api.bybit.com`).
#[derive(Debug, Clone)]
pub struct BybitLiveGateway {
    http: reqwest::Client,
    api_key: String,
    api_secret: String,
    base_url: String,
}

impl Loggable for BybitLiveGateway {
    const MODULE: &'static str = "qtss_execution::bybit_live";
}

impl BybitLiveGateway {
    /// Production REST host.
    pub fn mainnet(api_key: String, api_secret: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .build()
                .expect("reqwest client"),
            api_key,
            api_secret,
            base_url: "https://api.bybit.com".into(),
        }
    }

    fn dec_str(d: Decimal) -> String {
        d.normalize().to_string()
    }

    async fn post_v5_signed(&self, path: &str, body: &Value) -> Result<Value, ExecutionError> {
        let body_str =
            serde_json::to_string(body).map_err(|e| ExecutionError::Exchange(e.to_string()))?;
        let ts = Utc::now().timestamp_millis().to_string();
        let recv = "5000";
        let prehash = format!("{}{}{}{}", ts, self.api_key, recv, body_str);
        let mut mac = HmacSha256::new_from_slice(self.api_secret.as_bytes()).map_err(|_| {
            ExecutionError::Exchange("bybit: invalid API secret length".into())
        })?;
        mac.update(prehash.as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());

        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let resp = self
            .http
            .post(url)
            .header("Content-Type", "application/json")
            .header("X-BAPI-API-KEY", &self.api_key)
            .header("X-BAPI-SIGN", sig)
            .header("X-BAPI-TIMESTAMP", &ts)
            .header("X-BAPI-RECV-WINDOW", recv)
            .body(body_str)
            .send()
            .await
            .map_err(|e| ExecutionError::Exchange(format!("bybit HTTP: {e}")))?;

        let text = resp
            .text()
            .await
            .map_err(|e| ExecutionError::Exchange(format!("bybit body: {e}")))?;
        let v: Value =
            serde_json::from_str(&text).map_err(|e| ExecutionError::Exchange(format!(
                "bybit JSON: {e} (body starts {:.80})",
                text.chars().take(80).collect::<String>()
            )))?;

        let code = v.get("retCode").and_then(|x| x.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = v
                .get("retMsg")
                .and_then(|x| x.as_str())
                .unwrap_or("error");
            return Err(ExecutionError::Exchange(format!(
                "bybit retCode {code}: {msg}"
            )));
        }
        Ok(v)
    }

    fn bybit_linear_time_in_force(
        tf: TimeInForce,
        post_only: bool,
    ) -> Result<&'static str, ExecutionError> {
        if post_only {
            return Ok("PostOnly");
        }
        match tf {
            TimeInForce::Gtc => Ok("GTC"),
            TimeInForce::Ioc => Ok("IOC"),
            TimeInForce::Fok => Ok("FOK"),
            TimeInForce::Gtd => Err(ExecutionError::Exchange(
                "bybit linear: GTD time-in-force not supported".into(),
            )),
        }
    }

    /// `POST /v5/order/cancel` — identify order by client `orderLinkId` (UUID simple hex).
    pub async fn cancel_linear_by_order_link(
        &self,
        symbol: &str,
        order_link_id: &Uuid,
    ) -> Result<Value, ExecutionError> {
        let body = json!({
            "category": "linear",
            "symbol": symbol.trim().to_uppercase(),
            "orderLinkId": order_link_id.as_simple().to_string(),
        });
        self.post_v5_signed("/v5/order/cancel", &body).await
    }

    /// Same contract as [`crate::BinanceLiveGateway::place_with_venue_response`]: full API JSON as `venue_response`.
    pub async fn place_with_venue_response(
        &self,
        intent: OrderIntent,
    ) -> Result<(Uuid, Value), ExecutionError> {
        if intent.requires_human_approval {
            return Err(ExecutionError::PendingApproval);
        }
        if intent.instrument.exchange != ExchangeId::Bybit {
            return Err(ExecutionError::Exchange(
                "Bybit gateway: instrument.exchange must be bybit".into(),
            ));
        }
        if intent.instrument.segment != MarketSegment::Futures {
            return Err(ExecutionError::Exchange(
                "bybit live: only linear futures (USDT-M) is implemented".into(),
            ));
        }

        let symbol = intent.instrument.symbol.trim().to_uppercase();
        let id = Uuid::new_v4();
        let cid = id.as_simple().to_string();
        let side = match intent.side {
            OrderSide::Buy => "Buy",
            OrderSide::Sell => "Sell",
        };
        let reduce_only = intent
            .futures
            .as_ref()
            .and_then(|f| f.reduce_only)
            .unwrap_or(false);

        let body = match &intent.order_type {
            OrderType::Market => json!({
                "category": "linear",
                "symbol": symbol,
                "side": side,
                "orderType": "Market",
                "qty": Self::dec_str(intent.quantity),
                "reduceOnly": reduce_only,
                "orderLinkId": cid,
            }),
            OrderType::Limit {
                price,
                post_only,
            } => {
                let tif = Self::bybit_linear_time_in_force(intent.time_in_force, *post_only)?;
                json!({
                    "category": "linear",
                    "symbol": symbol,
                    "side": side,
                    "orderType": "Limit",
                    "qty": Self::dec_str(intent.quantity),
                    "price": Self::dec_str(*price),
                    "timeInForce": tif,
                    "reduceOnly": reduce_only,
                    "orderLinkId": cid,
                })
            }
            _ => {
                return Err(ExecutionError::Exchange(
                    "bybit live: only Market and Limit orders are implemented".into(),
                ));
            }
        };

        let venue = self.post_v5_signed("/v5/order", &body).await?;
        log_business(
            QtssLogLevel::Info,
            Self::MODULE,
            format!("bybit live place {symbol} orderLinkId={cid}"),
        );
        Ok((id, venue))
    }
}

#[async_trait]
impl ExecutionGateway for BybitLiveGateway {
    fn set_reference_price(
        &self,
        _instrument: &InstrumentId,
        _price: Decimal,
    ) -> Result<(), ExecutionError> {
        Ok(())
    }

    #[instrument(skip(self, intent))]
    async fn place(&self, intent: OrderIntent) -> Result<Uuid, ExecutionError> {
        self.place_with_venue_response(intent)
            .await
            .map(|(id, _)| id)
    }

    async fn cancel(&self, _client_order_id: Uuid) -> Result<(), ExecutionError> {
        Err(ExecutionError::Exchange(
            "Bybit: use cancel_linear_by_order_link(symbol, id) or POST /api/v1/orders/bybit/cancel"
                .into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn venue_order_id_from_nested_result() {
        let v = json!({"retCode":0,"result":{"orderId":"22001234567"}});
        assert_eq!(
            venue_order_id_from_bybit_v5_response(&v),
            Some(22_001_234_567)
        );
    }

    #[test]
    fn venue_order_id_top_level_fallback() {
        let v = json!({"orderId": 42});
        assert_eq!(venue_order_id_from_bybit_v5_response(&v), Some(42));
    }
}
