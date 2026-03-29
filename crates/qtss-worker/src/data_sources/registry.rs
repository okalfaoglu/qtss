//! Well-known `source_key` strings (worker + confluence). DB-backed HTTP keys are dynamic.
//!
//! PLAN Phase G — kayıtlı toplayıcıların tek listesi (dokümantasyon / ileride otomasyon).

/// Nansen token screener dual-write (`nansen_engine`); confluence smart-money pillar.
pub const NANSEN_TOKEN_SCREENER_DATA_KEY: &str = "nansen_token_screener";

#[derive(Debug, Clone, Copy)]
pub struct RegisteredDataSource {
    pub source_key: &'static str,
    pub provider_kind: &'static str,
    pub description: &'static str,
}

/// Worker’da sabit kayıtlı türler (`external_fetch` anahtarları dinamik).
pub const REGISTERED_DATA_SOURCES: &[RegisteredDataSource] = &[
    RegisteredDataSource {
        source_key: NANSEN_TOKEN_SCREENER_DATA_KEY,
        provider_kind: "NansenTokenScreenerProvider",
        description: "POST token screener; `DataSourceProvider`; `nansen_persist` → nansen_snapshots + data_snapshots; meta + qtss_fetch_duration_ms",
    },
    RegisteredDataSource {
        source_key: "*",
        provider_kind: "HttpGenericProvider",
        description: "Her `external_data_sources` satırı; GET/POST; meta: http_status, qtss_fetch_duration_ms",
    },
];
