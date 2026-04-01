//! OKX v5 **`ExecutionGateway`** — USDT **SWAP** (`instId` `*-USDT-SWAP`) **market** / **limit** (+ IOC, FOK, post-only) ve `cancel-order` (`clOrdId`). Requires API **passphrase**.
//! Spot / options: not wired (`QTSS_MASTER_DEV_GUIDE` §2.3.12).

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
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

const OKX_TRADE_ORDER_PATH: &str = "/api/v5/trade/order";
const OKX_TRADE_CANCEL_PATH: &str = "/api/v5/trade/cancel-order";

/// Maps `BTCUSDT` style symbol to OKX perpetual `instId`.
#[must_use]
pub fn okx_usdt_swap_inst_id(symbol: &str) -> String {
    let u = symbol.trim().to_uppercase();
    if let Some(base) = u.strip_suffix("USDT") {
        if !base.is_empty() {
            return format!("{base}-USDT-SWAP");
        }
    }
    u
}

/// Parses `POST /api/v5/trade/order` success body: `data[0].ordId`.
#[must_use]
pub fn venue_order_id_from_okx_v5_response(v: &Value) -> Option<i64> {
    let first = v.get("data").and_then(|d| d.as_array()).and_then(|a| a.first())?;
    first
        .get("ordId")
        .and_then(|x| x.as_i64())
        .or_else(|| first.get("ordId").and_then(|x| x.as_str()).and_then(|s| s.parse().ok()))
}

#[derive(Debug, Clone)]
pub struct OkxLiveGateway {
    http: reqwest::Client,
    api_key: String,
    api_secret: String,
    passphrase: String,
    base_url: String,
}

impl Loggable for OkxLiveGateway {
    const MODULE: &'static str = "qtss_execution::okx_live";
}

impl OkxLiveGateway {
    /// Production REST host (`https://www.okx.com`).
    pub fn mainnet(api_key: String, api_secret: String, passphrase: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .build()
                .expect("reqwest client"),
            api_key,
            api_secret,
            passphrase,
            base_url: "https://www.okx.com".into(),
        }
    }

    fn dec_str(d: Decimal) -> String {
        d.normalize().to_string()
    }

    async fn post_signed(
        &self,
        path: &str,
        body: &Value,
    ) -> Result<Value, ExecutionError> {
        let body_str =
            serde_json::to_string(body).map_err(|e| ExecutionError::Exchange(e.to_string()))?;
        let ts = Utc::now().timestamp_millis().to_string();
        let prehash = format!("{}{}{}{}", ts, "POST", path, body_str);
        let mut mac = HmacSha256::new_from_slice(self.api_secret.as_bytes()).map_err(|_| {
            ExecutionError::Exchange("okx: invalid API secret length".into())
        })?;
        mac.update(prehash.as_bytes());
        let sign = B64.encode(mac.finalize().into_bytes());

        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let resp = self
            .http
            .post(url)
            .header("Content-Type", "application/json")
            .header("OK-ACCESS-KEY", &self.api_key)
            .header("OK-ACCESS-SIGN", sign)
            .header("OK-ACCESS-TIMESTAMP", &ts)
            .header("OK-ACCESS-PASSPHRASE", &self.passphrase)
            .body(body_str)
            .send()
            .await
            .map_err(|e| ExecutionError::Exchange(format!("okx HTTP: {e}")))?;

        let text = resp
            .text()
            .await
            .map_err(|e| ExecutionError::Exchange(format!("okx body: {e}")))?;
        let v: Value =
            serde_json::from_str(&text).map_err(|e| ExecutionError::Exchange(format!(
                "okx JSON: {e} (body starts {:.80})",
                text.chars().take(80).collect::<String>()
            )))?;

        let code = v
            .get("code")
            .and_then(|x| x.as_str())
            .unwrap_or("-1");
        if code != "0" {
            let msg = v.get("msg").and_then(|x| x.as_str()).unwrap_or("error");
            return Err(ExecutionError::Exchange(format!("okx code {code}: {msg}")));
        }
        Ok(v)
    }

    fn okx_swap_ord_type_for_limit(
        post_only: bool,
        tif: TimeInForce,
    ) -> Result<&'static str, ExecutionError> {
        if post_only {
            if tif != TimeInForce::Gtc {
                return Err(ExecutionError::Exchange(
                    "okx: post_only limit requires time_in_force GTC".into(),
                ));
            }
            return Ok("post_only");
        }
        match tif {
            TimeInForce::Gtc => Ok("limit"),
            TimeInForce::Ioc => Ok("ioc"),
            TimeInForce::Fok => Ok("fok"),
            TimeInForce::Gtd => Err(ExecutionError::Exchange(
                "okx: GTD time-in-force not supported".into(),
            )),
        }
    }

    /// `POST /api/v5/trade/cancel-order` — `instId` + client `clOrdId` (UUID simple hex).
    pub async fn cancel_swap_by_cl_ord_id(
        &self,
        inst_id: &str,
        client_order_id: &Uuid,
    ) -> Result<Value, ExecutionError> {
        let body = json!({
            "instId": inst_id,
            "clOrdId": client_order_id.as_simple().to_string(),
        });
        self.post_signed(OKX_TRADE_CANCEL_PATH, &body).await
    }

    pub async fn place_with_venue_response(
        &self,
        intent: OrderIntent,
    ) -> Result<(Uuid, Value), ExecutionError> {
        if intent.requires_human_approval {
            return Err(ExecutionError::PendingApproval);
        }
        if self.passphrase.trim().is_empty() {
            return Err(ExecutionError::Exchange(
                "okx: passphrase is required (exchange_accounts.passphrase)".into(),
            ));
        }
        if intent.instrument.exchange != ExchangeId::Okx {
            return Err(ExecutionError::Exchange(
                "Okx gateway: instrument.exchange must be okx".into(),
            ));
        }
        if intent.instrument.segment != MarketSegment::Futures {
            return Err(ExecutionError::Exchange(
                "okx live: only USDT SWAP (futures segment) is implemented".into(),
            ));
        }

        let inst_id = okx_usdt_swap_inst_id(&intent.instrument.symbol);
        let id = Uuid::new_v4();
        let cid = id.as_simple().to_string();
        let side = match intent.side {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        };
        let reduce_only = intent
            .futures
            .as_ref()
            .and_then(|f| f.reduce_only)
            .unwrap_or(false);

        let body = match &intent.order_type {
            OrderType::Market => json!({
                "instId": inst_id,
                "tdMode": "cross",
                "side": side,
                "ordType": "market",
                "sz": Self::dec_str(intent.quantity),
                "reduceOnly": reduce_only,
                "clOrdId": cid,
            }),
            OrderType::Limit {
                price,
                post_only,
            } => {
                let ot = Self::okx_swap_ord_type_for_limit(*post_only, intent.time_in_force)?;
                json!({
                    "instId": inst_id,
                    "tdMode": "cross",
                    "side": side,
                    "ordType": ot,
                    "px": Self::dec_str(*price),
                    "sz": Self::dec_str(intent.quantity),
                    "reduceOnly": reduce_only,
                    "clOrdId": cid,
                })
            }
            _ => {
                return Err(ExecutionError::Exchange(
                    "okx live: only Market and Limit orders are implemented".into(),
                ));
            }
        };

        let venue = self.post_signed(OKX_TRADE_ORDER_PATH, &body).await?;
        log_business(
            QtssLogLevel::Info,
            Self::MODULE,
            format!("okx live place {inst_id} clOrdId={cid}"),
        );
        Ok((id, venue))
    }
}

#[async_trait]
impl ExecutionGateway for OkxLiveGateway {
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
            .map(|(uid, _)| uid)
    }

    async fn cancel(&self, _client_order_id: Uuid) -> Result<(), ExecutionError> {
        Err(ExecutionError::Exchange(
            "Okx: use cancel_swap_by_cl_ord_id(inst_id, id) or POST /api/v1/orders/okx/cancel".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn okx_swap_inst_id_from_concat_symbol() {
        assert_eq!(okx_usdt_swap_inst_id("BTCUSDT"), "BTC-USDT-SWAP");
        assert_eq!(okx_usdt_swap_inst_id("ethusdt"), "ETH-USDT-SWAP");
    }

    #[test]
    fn venue_order_id_from_data_array() {
        let v = json!({"code":"0","data":[{"ordId":"987654321"}]});
        assert_eq!(venue_order_id_from_okx_v5_response(&v), Some(987_654_321));
    }
}
