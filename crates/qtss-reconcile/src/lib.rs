//! qtss-reconcile — broker truth vs engine state mutabakatı.
//!
//! Two layers ship from this crate:
//!
//! - **v1 (legacy)**: Binance açık emir mutabakatı sonrası
//!   `exchange_orders` güncellemeleri. v1 store ile konuşur, eski
//!   worker tarafından çağrılır. Faz 7'de düşürülecek.
//! - **v2** (`mod v2`): venue-agnostik snapshot diff'i. Bir broker
//!   tarafı (`BrokerSnapshot`) ile portfolio engine tarafı
//!   (`EngineSnapshot`) verilir, [`reconcile`] eksik/fazla pozisyon ve
//!   açık emirleri çıkartır. Adapter'lara veya store'a hiç bağlı
//!   değildir — saf veri (CLAUDE.md rule #3).

mod binance_open_orders_patch;
pub mod v2;

pub use binance_open_orders_patch::{
    apply_binance_futures_open_orders_patch, apply_binance_spot_open_orders_patch,
    BinanceOpenOrdersPatchConfig,
};

pub use v2::{
    reconcile, BrokerOpenOrder, BrokerPosition, BrokerSnapshot, DriftSeverity,
    EngineOpenOrder, EnginePosition, EngineSnapshot, OrderDrift, OrderDriftSide,
    PositionDrift, ReconcileReport, ReconcileTolerance,
};
