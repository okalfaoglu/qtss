use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::symbol::InstrumentId;

/// Taşınabilir emir yönü — JSON/API ve backtest (`qtss-backtest`) bu tipi kullanır (`buy` / `sell`).
///
/// Binance REST `side` (`BUY` / `SELL`) için tek dönüşüm: `qtss_binance::types::OrderSide::from`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeInForce {
    Gtc,
    Ioc,
    Fok,
    Gtd,
}

/// Tüm emir tipleri şemada mevcut; **hemen uygulanacak** olanlar spot/futures market/limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrderType {
    Market,
    Limit {
        price: Decimal,
        post_only: bool,
    },
    StopMarket {
        stop_price: Decimal,
    },
    StopLimit {
        stop_price: Decimal,
        limit_price: Decimal,
    },
    TakeProfitMarket {
        stop_price: Decimal,
    },
    TakeProfitLimit {
        stop_price: Decimal,
        limit_price: Decimal,
    },
    TrailingStopMarket {
        /// Callback oranı (borsa tanımına göre yüzde veya fiyat farkı adapter’da çevrilir).
        callback_rate: Decimal,
    },
    TrailingStopLimit {
        callback_rate: Decimal,
        limit_offset: Decimal,
    },
    /// Iceberg / hidden miktar — borsa desteğine göre adapter composite emir yapabilir.
    Iceberg {
        display_quantity: Decimal,
        limit_price: Decimal,
    },
    Oco {
        /// İki bacaklı emir; uygulama katmanında iki child id tutulur.
        primary: Box<OrderType>,
        secondary: Box<OrderType>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderIntent {
    pub instrument: InstrumentId,
    pub side: OrderSide,
    pub quantity: Decimal,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    /// İnsan onayı gerektiren strateji/AI modu için meta.
    pub requires_human_approval: bool,
}
