//! Binance spot + USDT-M **`ExecutionGateway`** — API anahtarı ile gerçek emir.

use async_trait::async_trait;
use qtss_binance::{
    BinanceClient, FuturesOrderType, OrderSide as BSide, SpotOrderType, TimeInForce as BTif,
};
use qtss_common::{log_business, Loggable, QtssLogLevel};
use qtss_domain::exchange::{ExchangeId, MarketSegment};
use qtss_domain::orders::{OrderIntent, OrderType, TimeInForce};
use qtss_domain::symbol::InstrumentId;
use rust_decimal::Decimal;
use serde_json::Value;
use tracing::instrument;
use uuid::Uuid;

use crate::gateway::{ExecutionError, ExecutionGateway};

pub struct BinanceLiveGateway {
    client: std::sync::Arc<BinanceClient>,
}

impl Loggable for BinanceLiveGateway {
    const MODULE: &'static str = "qtss_execution::binance_live";
}

impl BinanceLiveGateway {
    pub fn new(client: std::sync::Arc<BinanceClient>) -> Self {
        Self { client }
    }

    /// Borsa JSON yanıtı ile birlikte (`orderId` çıkarımı için).
    pub async fn place_with_venue_response(
        &self,
        intent: OrderIntent,
    ) -> Result<(Uuid, Value), ExecutionError> {
        if intent.requires_human_approval {
            return Err(ExecutionError::PendingApproval);
        }
        if intent.instrument.exchange != ExchangeId::Binance {
            return Err(ExecutionError::Exchange(
                "yalnızca Binance enstrümanları destekleniyor".into(),
            ));
        }
        let symbol = intent.instrument.symbol.clone();
        let id = Uuid::new_v4();
        let cid = id.as_simple().to_string();

        let venue = match intent.instrument.segment {
            MarketSegment::Spot => self.place_spot(&intent, &symbol, &cid).await?,
            MarketSegment::Futures => self.place_futures(&intent, &symbol, &cid).await?,
            MarketSegment::Margin | MarketSegment::Options => {
                return Err(ExecutionError::Exchange(
                    "bu piyasa segmenti için canlı emir kapalı".into(),
                ));
            }
        };

        log_business(
            QtssLogLevel::Info,
            Self::MODULE,
            format!("binance live place {} cid={}", symbol, cid),
        );
        Ok((id, venue))
    }

    fn dec_str(d: Decimal) -> String {
        d.normalize().to_string()
    }

    fn map_tif(t: TimeInForce) -> Result<BTif, ExecutionError> {
        match t {
            TimeInForce::Gtc => Ok(BTif::Gtc),
            TimeInForce::Ioc => Ok(BTif::Ioc),
            TimeInForce::Fok => Ok(BTif::Fok),
            TimeInForce::Gtd => Err(ExecutionError::Exchange(
                "GTD Binance canlı emrinde desteklenmiyor".into(),
            )),
        }
    }

    fn futures_position_side_reduce(intent: &OrderIntent) -> (Option<&str>, Option<bool>) {
        let Some(f) = intent.futures.as_ref() else {
            return (None, None);
        };
        let pos = f
            .position_side
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        (pos, f.reduce_only)
    }

    async fn place_spot(
        &self,
        intent: &OrderIntent,
        symbol: &str,
        cid: &str,
    ) -> Result<Value, ExecutionError> {
        let side: BSide = intent.side.into();
        let qty = Self::dec_str(intent.quantity);
        match &intent.order_type {
            OrderType::Market => self
                .client
                .spot_new_order(
                    symbol,
                    side,
                    SpotOrderType::Market,
                    None,
                    Some(&qty),
                    None,
                    None,
                    Some(cid),
                    None,
                    None,
                    None,
                )
                .await
                .map_err(|e| ExecutionError::Exchange(e.to_string())),
            OrderType::Limit { price, post_only } => {
                let (otype, tif): (SpotOrderType, Option<BTif>) = if *post_only {
                    (SpotOrderType::LimitMaker, None)
                } else {
                    (SpotOrderType::Limit, Some(Self::map_tif(intent.time_in_force)?))
                };
                let price_s = Self::dec_str(*price);
                self.client
                    .spot_new_order(
                        symbol,
                        side,
                        otype,
                        tif,
                        Some(&qty),
                        None,
                        Some(&price_s),
                        Some(cid),
                        None,
                        None,
                        None,
                    )
                    .await
                    .map_err(|e| ExecutionError::Exchange(e.to_string()))
            }
            _ => Err(ExecutionError::Exchange(
                "spot için bu emir tipi henüz bağlanmadı".into(),
            )),
        }
    }

    async fn place_futures(
        &self,
        intent: &OrderIntent,
        symbol: &str,
        cid: &str,
    ) -> Result<Value, ExecutionError> {
        let side: BSide = intent.side.into();
        let qty = Self::dec_str(intent.quantity);
        let (position_side, reduce_only) = Self::futures_position_side_reduce(intent);

        match &intent.order_type {
            OrderType::Market => self
                .client
                .fapi_new_order(
                    symbol,
                    side,
                    position_side,
                    FuturesOrderType::Market,
                    None,
                    Some(&qty),
                    reduce_only,
                    None,
                    Some(cid),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .await
                .map_err(|e| ExecutionError::Exchange(e.to_string())),
            OrderType::Limit { price, post_only } => {
                let tif = if *post_only {
                    BTif::Gtx
                } else {
                    Self::map_tif(intent.time_in_force)?
                };
                let price_s = Self::dec_str(*price);
                self.client
                    .fapi_new_order(
                        symbol,
                        side,
                        position_side,
                        FuturesOrderType::Limit,
                        Some(tif),
                        Some(&qty),
                        reduce_only,
                        Some(&price_s),
                        Some(cid),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .await
                    .map_err(|e| ExecutionError::Exchange(e.to_string()))
            }
            _ => Err(ExecutionError::Exchange(
                "futures için bu emir tipi henüz bağlanmadı".into(),
            )),
        }
    }
}

#[async_trait]
impl ExecutionGateway for BinanceLiveGateway {
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
            "Binance canlı iptal: HTTP /orders/binance/cancel ile symbol ve segment gönderin"
                .into(),
        ))
    }
}
