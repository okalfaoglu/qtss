//! Ortak istek yardımcıları (Binance alan adları camelCase / SCREAMING).

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub enum OrderSide {
    Buy,
    Sell,
}

impl OrderSide {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Sell => "SELL",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SpotOrderType {
    Limit,
    Market,
    StopLoss,
    StopLossLimit,
    TakeProfit,
    TakeProfitLimit,
    LimitMaker,
}

impl SpotOrderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Limit => "LIMIT",
            Self::Market => "MARKET",
            Self::StopLoss => "STOP_LOSS",
            Self::StopLossLimit => "STOP_LOSS_LIMIT",
            Self::TakeProfit => "TAKE_PROFIT",
            Self::TakeProfitLimit => "TAKE_PROFIT_LIMIT",
            Self::LimitMaker => "LIMIT_MAKER",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FuturesOrderType {
    Limit,
    Market,
    Stop,
    StopMarket,
    TakeProfit,
    TakeProfitMarket,
    TrailingStopMarket,
}

impl FuturesOrderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Limit => "LIMIT",
            Self::Market => "MARKET",
            Self::Stop => "STOP",
            Self::StopMarket => "STOP_MARKET",
            Self::TakeProfit => "TAKE_PROFIT",
            Self::TakeProfitMarket => "TAKE_PROFIT_MARKET",
            Self::TrailingStopMarket => "TRAILING_STOP_MARKET",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TimeInForce {
    Gtc,
    Ioc,
    Fok,
    Gtx,
}

impl TimeInForce {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gtc => "GTC",
            Self::Ioc => "IOC",
            Self::Fok => "FOK",
            Self::Gtx => "GTX",
        }
    }
}

pub fn insert_opt(map: &mut BTreeMap<String, String>, key: &str, v: Option<&str>) {
    if let Some(s) = v {
        if !s.is_empty() {
            map.insert(key.into(), s.into());
        }
    }
}
