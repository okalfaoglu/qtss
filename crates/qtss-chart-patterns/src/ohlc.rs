//! Trendoscope Pine v6 **`Trendoscope/ohlc`** — `export type OHLC` ile alan eşlemesi.
//!
//! # Taşınan alt küme (`OhlcBar`)
//!
//! | Pine `OHLC` alanı | QTSS `OhlcBar` | Not |
//! |---|---:|---|
//! | `o` | `open` | |
//! | `h` | `high` | |
//! | `l` | `low` | |
//! | `c` | `close` | |
//! | `barindex` | `bar_index` | API JSON’da `bar_index` (tarama diliminde 0..n-1). |
//!
//! # Bilinçli olarak yok (şimdilik)
//!
//! - **`highBeforeLow` / `highAfterLow` / `lowBeforeHigh` / `lowAfterHigh`** — alt zaman dilimi içi yol; varsayılan Pine’da `high`/`low` ile aynı. Kanal / zigzag motoru tek zaman dilimi OHLC ile çalışır.
//! - **`bartime`** — web tarafında `open_time` ISO’dan saniye üretilir; sunucu taramasında zaman ekseni `bar_index` + mum haritasıdır.
//! - **`indicators` / `Indicator` / `getPoints` / `plot`** — çizim ve gösterge dizileri; Rust’ta `PatternDrawingBatch` + web LWC.
//! - **`getOhlcArray`**, **`push`/`unshift` (maxItems)** — Pine `var array` + boyut sınırı; QTSS’te HTTP gövdesi `Vec<OhlcBar>` ve `calculated_bars` kırpması uygulama katmanında.
//!
//! ACP göstergesi zigzaga çoğu zaman `array.from(highSource, lowSource)` verir; tam `OHLC` geniş alanları zorunlu değildir.

use serde::{Deserialize, Serialize};

/// Bir mumun OHLC + tarama dilimindeki bar indeksi (Pine `OHLC.barindex` / `chart.point.index`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OhlcBar {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub bar_index: i64,
    /// Mum hacmi. `None` ise hacim verisi mevcut değil (eski istemciler / birim testler).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
}
