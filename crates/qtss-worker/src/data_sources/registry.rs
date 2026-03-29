//! Well-known `source_key` strings (worker + confluence). DB-backed HTTP keys are dynamic.
//!
//! PLAN Phase G — kayıtlı toplayıcıların tek listesi (dokümantasyon / ileride otomasyon).

/// Nansen token screener dual-write (`nansen_engine`); confluence smart-money pillar.
pub const NANSEN_TOKEN_SCREENER_DATA_KEY: &str = "nansen_token_screener";

/// Optional `data_snapshots` keys when worker persists smart-money / TGM endpoints (dev guide ADIM 3–5).
pub const NANSEN_NETFLOWS_DATA_KEY: &str = "nansen_netflows";
pub const NANSEN_PERP_TRADES_DATA_KEY: &str = "nansen_perp_trades";
pub const NANSEN_FLOW_INTELLIGENCE_DATA_KEY: &str = "nansen_flow_intelligence";
/// Alias for dev-guide naming (`NANSEN_FLOW_INTEL_DATA_KEY`).
pub const NANSEN_FLOW_INTEL_DATA_KEY: &str = NANSEN_FLOW_INTELLIGENCE_DATA_KEY;
pub const NANSEN_WHO_BOUGHT_SOLD_DATA_KEY: &str = "nansen_who_bought_sold";
/// Alias for dev-guide naming (`NANSEN_WHO_BOUGHT_DATA_KEY`).
pub const NANSEN_WHO_BOUGHT_DATA_KEY: &str = NANSEN_WHO_BOUGHT_SOLD_DATA_KEY;
pub const NANSEN_HOLDINGS_DATA_KEY: &str = "nansen_holdings";
/// Merged profiler perp-positions across whale watchlist → on-chain `hl_whale_score`.
pub const NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY: &str = "nansen_whale_perp_aggregate";
pub const NANSEN_WHALE_WATCHLIST_CONFIG_KEY: &str = "nansen_whale_watchlist";

pub const REGISTERED_NANSEN_HTTP_KEYS: &[&str] = &[
    NANSEN_TOKEN_SCREENER_DATA_KEY,
    NANSEN_NETFLOWS_DATA_KEY,
    NANSEN_HOLDINGS_DATA_KEY,
    NANSEN_PERP_TRADES_DATA_KEY,
    NANSEN_FLOW_INTELLIGENCE_DATA_KEY,
    NANSEN_WHO_BOUGHT_SOLD_DATA_KEY,
    NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY,
];

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
        source_key: NANSEN_NETFLOWS_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "`nansen_extended` → `data_snapshots` smart-money/netflows",
    },
    RegisteredDataSource {
        source_key: NANSEN_HOLDINGS_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "`nansen_extended` → `data_snapshots` smart-money/holdings",
    },
    RegisteredDataSource {
        source_key: NANSEN_PERP_TRADES_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "`nansen_extended` → `data_snapshots` smart-money/perp-trades",
    },
    RegisteredDataSource {
        source_key: NANSEN_FLOW_INTELLIGENCE_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "`nansen_extended` → `data_snapshots` tgm/flow-intelligence (config `nansen_flow_intel_by_symbol`)",
    },
    RegisteredDataSource {
        source_key: NANSEN_WHO_BOUGHT_SOLD_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "`nansen_extended` → `data_snapshots` tgm/who-bought-sold",
    },
    RegisteredDataSource {
        source_key: NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "`nansen_extended` whale watchlist → merged profiler/perp-positions",
    },
    RegisteredDataSource {
        source_key: "*",
        provider_kind: "HttpGenericProvider",
        description: "Her `external_data_sources` satırı; GET/POST; meta: http_status, qtss_fetch_duration_ms",
    },
];
