//! Faz 9.0.2 — Feature extractor implementations (ConfluenceSource impls).
//!
//! Her module tek `ConfluenceSource` impl'i içerir. Yeni kaynak eklemek:
//!   1. `pub mod <name>;` buraya ekle.
//!   2. `crate::feature_store::FEATURE_SOURCES` array'ine satır ekle.
//!   3. migration 0116'daki config key listesine `<name>.enabled` ekle.

pub mod derivatives;
pub mod wyckoff;
