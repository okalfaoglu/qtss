//! Emir yürütme: **çalışma moduna** göre gateway seçimi.
//!
//! | [`qtss_domain::execution::ExecutionMode`] | Veri | Yürütme | Tipik gateway |
//! |---|---|---|---|
//! | **Live** | Borsa canlı | Gerçek emir | [`BinanceLiveGateway`]; [`BybitLiveGateway`]; [`OkxLiveGateway`] (USDT linear/SWAP market+limit, cancel); diğer: [`UnsupportedLiveGateway`] |
//! | **Dry** | Borsa canlı (paper) | Simüle, sanal bakiye | [`DryRunGateway`] (+ DB kaydı uygulama katmanında) |
//! | **Backtest** | Geçmiş bar | Simüle | `qtss_backtest::BacktestEngine` |
//!
//! Komisyon: tüm modlarda [`qtss_domain::commission::CommissionPolicy`] + isteğe bağlı
//! [`qtss_domain::commission::CommissionResolver`] (borsa API); API yoksa manuel bps.

mod binance_live;
mod binance_order_status;
mod bybit_live;
mod okx_live;
mod dry;
mod gateway;
mod live;
mod reconcile;
mod unsupported_live;

pub use binance_live::BinanceLiveGateway;
pub use bybit_live::{venue_order_id_from_bybit_v5_response, BybitLiveGateway};
pub use okx_live::{
    okx_usdt_swap_inst_id, venue_order_id_from_okx_v5_response, OkxLiveGateway,
};
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
pub use unsupported_live::UnsupportedLiveGateway;

pub use qtss_domain::execution::{ExecutionMode, VirtualLedgerParams};
pub use qtss_domain::commission::{commission_fee, rate_from_bps, CommissionPolicy};
