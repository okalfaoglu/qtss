//! Algoritmik işlem motorunun **çalışma modları** — veri kaynağı, yürütme ve kayıt ayrımı.
//!
//! # Live (canlı işlem)
//! - **Veri:** Borsadan (ör. Binance) anlık gerçek zamanlı akış.
//! - **Yürütme:** Sinyaller gerçek emir olarak borsaya gider; gerçek bakiye kullanılır.
//! - **Kayıt:** Gerçekleşen emirler, durumlar ve portföy değişiklikleri DB’de tutulur (uygulama katmanı).
//!
//! # Dry (canlı simülasyon / paper trading)
//! - **Veri:** Live ile aynı — canlı piyasa verisi.
//! - **Yürütme:** Emirler borsaya **gönderilmez**; motor içinde sanal bakiye üzerinden simüle edilir.
//! - **Kayıt:** Simüle işlemler DB’ye yazılarak risksiz performans izlenir (uygulama katmanı).
//!
//! # Backtest (geçmiş veri testi)
//! - **Veri:** Seçilen tarih aralığında geçmiş OHLC / bar akışı.
//! - **Yürütme:** Algoritma hızlı replay; işlemler sanal bakiye ile simüle edilir ([`crate::bar::TimestampBar`] tabanlı motor: `qtss-backtest`).
//! - **Kayıt:** Drawdown, kar/zarar, işlem listesi gibi metrikler üretilir.
//!
//! # Ortak kurallar
//! - **Komisyon:** Üç modda da işlem maliyeti yansıtılır — [`super::commission::CommissionPolicy`].
//! - **Sanal bakiye:** Dry ve Backtest için [`VirtualLedgerParams`] ile başlangıç nakdi tanımlanır.
//! - **Binance USDT-M:** Canlı yürütmede [`super::orders::OrderIntent::futures`] (`position_side`, `reduce_only`) ve ayrı API ile kaldıraç (`fapi/v1/leverage`).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Motorun hangi ortamda çalıştığı — gateway ve veri kaynağı seçimini belirler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    /// Gerçek emir + gerçek bakiye.
    Live,
    /// Canlı veri + sanal yürütme (paper).
    Dry,
    /// Geçmiş veri + sanal yürütme.
    Backtest,
}

/// Dry ve backtest için izole sanal kasa başlangıcı (tek quote cinsinden MVP).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualLedgerParams {
    /// Başlangıç nakit bakiyesi (ör. USDT).
    pub initial_quote_balance: Decimal,
}
