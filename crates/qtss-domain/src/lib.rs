//! Domain model: borsalar, sembol, mum/timestamp bar, emir tipleri, copy-trade, çalışma modları.
//!
//! ## Çalışma modları (`execution`)
//! - **Live / Dry / Backtest** — veri kaynağı ve yürütme ayrımı; komisyon politikası [`commission::CommissionPolicy`].
//!
//! ## Copy trade mantığı (özet)
//! - **Leader**: gerçekleşen işlemleri veya sinyalleri yayınlar.
//! - **Follower**: abonelik kurallarına göre (çoğaltma, max slippage, gecikme, min/max notional)
//!   aynı yönde pozisyon açar/kapatır.
//! - **Risk**: follower tarafında günlük zarar limiti, sembol whitelist, kasa yüzdesi.
//! - Uygulama: `copy_subscriptions` + `copy_execution_queue` (veya event bus) ile
//!   leader fill’lerinden türetilen hedef emirler; **slippage** ve **latency** ayrı metrik.

pub mod bar;
pub mod commission;
pub mod copy_trade;
pub mod exchange;
pub mod execution;
pub mod orders;
pub mod symbol;
pub mod tenancy;

pub use bar::{TimestampBar, TimestampBarFeed};
pub use commission::{
    commission_fee, rate_from_bps, CommissionPolicy, CommissionQuote, CommissionResolver,
    CommissionSource,
};
pub use execution::{ExecutionMode, VirtualLedgerParams};
pub use copy_trade::{CopyRule, CopySubscription, LeaderId};
pub use exchange::{ExchangeCapability, ExchangeId, MarketSegment};
pub use orders::{
    FuturesExecutionExtras, OrderIntent, OrderSide, OrderStatus, OrderType, TimeInForce,
};
pub use symbol::InstrumentId;
pub use tenancy::{OrganizationId, TenantContext};
