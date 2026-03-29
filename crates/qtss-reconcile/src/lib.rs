//! Binance açık emir mutabakatı sonrası `exchange_orders` güncellemeleri (ortak API + worker).

mod binance_open_orders_patch;

pub use binance_open_orders_patch::{
    apply_binance_futures_open_orders_patch, apply_binance_spot_open_orders_patch,
    BinanceOpenOrdersPatchConfig,
};
