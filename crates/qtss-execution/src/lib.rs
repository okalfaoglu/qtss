//! Emir yürütme: **çalışma moduna** göre gateway seçimi.
//!
//! | [`qtss_domain::execution::ExecutionMode`] | Veri | Yürütme | Tipik gateway |
//! |---|---|---|---|
//! | **Live** | Borsa canlı | Gerçek emir | [`BinanceLiveGateway`] |
//! | **Dry** | Borsa canlı (paper) | Simüle, sanal bakiye | [`DryRunGateway`] (+ DB kaydı uygulama katmanında) |
//! | **Backtest** | Geçmiş bar | Simüle | `qtss_backtest::BacktestEngine` |
//!
//! Komisyon: tüm modlarda [`qtss_domain::commission::CommissionPolicy`] + isteğe bağlı
//! [`qtss_domain::commission::CommissionResolver`] (borsa API); API yoksa manuel bps.

mod binance_live;
mod binance_order_status;
mod dry;
mod gateway;
mod live;
mod reconcile;

pub use binance_live::BinanceLiveGateway;
pub use binance_order_status::exchange_order_status_from_binance_json;
pub use dry::{
    apply_place, instrument_position_key, DryLedgerState, DryPlaceOutcome, DryRunGateway,
};
pub use gateway::{ExecutionError, ExecutionGateway, FillEvent};
pub use live::LiveGateway;
pub use reconcile::{
    reconcile_binance_futures_open_orders, reconcile_binance_spot_open_orders,
    venue_order_ids_submitted_not_on_open_list, ExchangeOrderVenueSnapshot, ReconcileReport,
};

pub use qtss_domain::commission::{commission_fee, rate_from_bps, CommissionPolicy};
pub use qtss_domain::execution::{ExecutionMode, VirtualLedgerParams};
