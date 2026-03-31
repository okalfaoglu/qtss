//! Binance **Spot** ve **USDT-M Futures (FAPI)** REST istemcisi + katalog senkronu.
//!
//! - Halka açık uçlar API anahtarı gerektirmez.
//! - Emir ve hesap uçları için [`BinanceClientConfig::mainnet_with_keys`].
//! - Testnet: [`BinanceEndpoints::testnet`] ile [`BinanceClientConfig`] oluşturun.

mod commission;
mod config;
mod error;
mod futures;
mod klines;
mod order_parse;
mod rest;
mod sign;
mod spot;
mod types;
pub mod ws;
pub mod ws_kline;

pub mod catalog_sync;

pub use catalog_sync::{sync_full_binance_catalog, sync_spot_instruments, sync_usdt_futures_instruments, CatalogSyncStats};
pub use commission::{
    commission_rate_from_fapi_response,
    default_spot_commission_bps, default_usdt_futures_commission_bps,
    futures_commission_hint_from_exchange_info, resolve_from_exchange_info_stub,
    spot_commission_hint_from_exchange_info, trade_fee_from_sapi_response, CommissionBps,
};
pub use config::{BinanceClientConfig, BinanceCredentials, BinanceEndpoints};
pub use error::BinanceError;
pub use klines::{parse_klines_json, KlineBar};
pub use order_parse::venue_order_id_from_binance_order_response;
pub use ws::{
    connect_url, public_spot_combined_kline_url, public_spot_kline_url, public_usdm_combined_kline_url,
    public_usdm_kline_url, spot_user_data_stream_url, usdm_user_data_stream_url, WsStream,
};
pub use ws_kline::{parse_closed_kline_json, ClosedKline};
pub use types::{
    insert_opt, FuturesOrderType, OrderSide, SpotOrderType, TimeInForce,
};

use rest::RestCore;

/// Binance REST istemcisi.
pub struct BinanceClient {
    pub(crate) core: RestCore,
    pub(crate) cfg: BinanceClientConfig,
}

impl BinanceClient {
    pub fn new(cfg: BinanceClientConfig) -> Result<Self, BinanceError> {
        let core = RestCore::new(cfg.recv_window_ms)?;
        Ok(Self { core, cfg })
    }

    pub fn config(&self) -> &BinanceClientConfig {
        &self.cfg
    }
}
