//! Pure functions that translate v2 [`OrderRequest`] values into the
//! Binance wire vocabulary. Kept free of IO so they're trivially
//! testable and the adapter file stays a thin REST wrapper.

use crate::error::{BinanceExecError, BinanceExecResult};
use qtss_binance::{OrderSide as BSide, SpotOrderType, TimeInForce as BTif};
use qtss_domain::v2::intent::{
    OrderRequest, OrderType, Side, TimeInForce as VTif,
};

pub fn map_side(side: Side) -> BSide {
    match side {
        Side::Long => BSide::Buy,
        Side::Short => BSide::Sell,
    }
}

pub fn map_tif(tif: VTif) -> BTif {
    match tif {
        VTif::Gtc => BTif::Gtc,
        VTif::Ioc => BTif::Ioc,
        VTif::Fok => BTif::Fok,
        VTif::Day | VTif::Gtd => BTif::Gtc, // binance spot has no DAY/GTD
    }
}

/// (binance order type, needs price?, needs stop_price?)
fn order_type_dispatch(req: &OrderRequest) -> BinanceExecResult<SpotOrderType> {
    match (req.order_type, req.post_only) {
        (OrderType::Market, _) => Ok(SpotOrderType::Market),
        (OrderType::Limit, true) => Ok(SpotOrderType::LimitMaker),
        (OrderType::Limit, false) => Ok(SpotOrderType::Limit),
        (OrderType::Stop, _) => Ok(SpotOrderType::StopLoss),
        (OrderType::StopLimit, _) => Ok(SpotOrderType::StopLossLimit),
        (other, _) => Err(BinanceExecError::UnsupportedOrderType(format!(
            "{other:?}"
        ))),
    }
}

/// Build the (type, side, tif, qty, price, stop_price) tuple ready
/// for `BinanceClient::spot_new_order`.
pub struct SpotPayload {
    pub bn_type: SpotOrderType,
    pub side: BSide,
    pub tif: Option<BTif>,
    pub quantity: String,
    pub price: Option<String>,
    pub stop_price: Option<String>,
    pub client_order_id: String,
}

pub fn build_spot_payload(req: &OrderRequest) -> BinanceExecResult<SpotPayload> {
    let bn_type = order_type_dispatch(req)?;
    let needs_price = matches!(
        bn_type,
        SpotOrderType::Limit | SpotOrderType::LimitMaker | SpotOrderType::StopLossLimit
    );
    let needs_stop = matches!(
        bn_type,
        SpotOrderType::StopLoss | SpotOrderType::StopLossLimit
    );
    if needs_price && req.price.is_none() {
        return Err(BinanceExecError::UnsupportedOrderType(
            "limit-style order missing price".into(),
        ));
    }
    if needs_stop && req.stop_price.is_none() {
        return Err(BinanceExecError::UnsupportedOrderType(
            "stop-style order missing stop_price".into(),
        ));
    }
    // GTC is required by Binance for any LIMIT/STOP_LOSS_LIMIT order.
    let tif = if matches!(
        bn_type,
        SpotOrderType::Limit | SpotOrderType::StopLossLimit
    ) {
        Some(map_tif(req.time_in_force))
    } else {
        None
    };
    Ok(SpotPayload {
        bn_type,
        side: map_side(req.side),
        tif,
        quantity: req.quantity.normalize().to_string(),
        price: req.price.map(|p| p.normalize().to_string()),
        stop_price: req.stop_price.map(|p| p.normalize().to_string()),
        client_order_id: req.client_order_id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use qtss_domain::v2::instrument::{
        AssetClass, Instrument, SessionCalendar, Venue,
    };
    use qtss_domain::v2::intent::OrderRequest;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn req(order_type: OrderType, post_only: bool) -> OrderRequest {
        OrderRequest {
            client_order_id: Uuid::new_v4(),
            instrument: Instrument {
                venue: Venue::Binance,
                asset_class: AssetClass::CryptoSpot,
                symbol: "BTCUSDT".into(),
                quote_ccy: "USDT".into(),
                tick_size: dec!(0.01),
                lot_size: dec!(0.00001),
                session: SessionCalendar::binance_24x7(),
            },
            side: Side::Long,
            order_type,
            quantity: dec!(0.5),
            price: Some(dec!(50000)),
            stop_price: Some(dec!(49000)),
            time_in_force: VTif::Gtc,
            reduce_only: false,
            post_only,
            intent_id: None,
        }
    }

    #[test]
    fn market_has_no_tif_no_price() {
        let p = build_spot_payload(&req(OrderType::Market, false)).unwrap();
        assert!(matches!(p.bn_type, SpotOrderType::Market));
        assert!(p.tif.is_none());
    }

    #[test]
    fn post_only_limit_becomes_limit_maker() {
        let p = build_spot_payload(&req(OrderType::Limit, true)).unwrap();
        assert!(matches!(p.bn_type, SpotOrderType::LimitMaker));
        assert!(p.tif.is_none()); // LIMIT_MAKER must not send timeInForce
    }

    #[test]
    fn limit_carries_gtc() {
        let p = build_spot_payload(&req(OrderType::Limit, false)).unwrap();
        assert!(matches!(p.bn_type, SpotOrderType::Limit));
        assert!(p.tif.is_some());
    }
}
