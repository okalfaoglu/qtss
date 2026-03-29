//! Pluggable market data providers (F7 / PLAN §3, Phase G).
//!
//! **Worker’da kayıtlı toplayıcılar**
//! - [`HttpGenericProvider`](http_generic::HttpGenericProvider) — `engines::external_*` döngüleri, satır başına DB’den.
//! - [`NansenTokenScreenerProvider`](nansen_token_screener_provider::NansenTokenScreenerProvider) — `nansen_engine` döngüsü.
//!
//! Ham yanıtlar `data_snapshots`; Nansen ayrıca `nansen_persist` ile `nansen_snapshots`. Skor: [`crate::signal_scorer`].

pub mod http_generic;
pub mod nansen_persist;
pub mod nansen_token_screener_provider;
pub mod persist;
pub mod provider;
pub mod registry;
