//! Arka plan **veri motorları** — çıktı çoğunlukla `data_snapshots` (tek tablo, `source_key` ile ayrışır).
//!
//! HTTP `external_data_sources` satırları ailelere bölünerek ayrı `tokio` görevlerinde çalıştırılır; böylece
//! log etiketi, operasyonel kapatma ve ileride aile başına farklı poll süresi eklemek kolaylaşır.
//! Davranış: her motor kendi filtresine uyan satırları çeker; çakışan filtre olmamalıdır.

mod external_binance;
mod external_coinglass;
mod external_common;
mod external_hyperliquid;
mod external_misc;

pub use external_binance::run as external_binance_loop;
pub use external_coinglass::run as external_coinglass_loop;
pub use external_hyperliquid::run as external_hyperliquid_loop;
pub use external_misc::run as external_misc_loop;
