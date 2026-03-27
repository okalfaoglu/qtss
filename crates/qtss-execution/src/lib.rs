//! Emir yürütme: canlı borsa veya dry-run sanal defter.

mod binance_live;
mod dry;
mod gateway;
mod live;
mod reconcile;

pub use binance_live::BinanceLiveGateway;
pub use dry::DryRunGateway;
pub use gateway::{ExecutionError, ExecutionGateway, FillEvent};
pub use live::LiveGateway;
pub use reconcile::{
    reconcile_binance_spot_open_orders, ExchangeOrderVenueSnapshot, ReconcileReport,
};
