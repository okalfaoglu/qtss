//! Well-known `source_key` strings (worker + confluence). DB-backed HTTP keys are dynamic.
//!
//! PLAN Phase G — kayıtlı toplayıcıların tek listesi (dokümantasyon / ileride otomasyon).

/// Nansen token screener dual-write (`nansen_engine`); confluence smart-money pillar.
pub const NANSEN_TOKEN_SCREENER_DATA_KEY: &str = "nansen_token_screener";

/// Optional `data_snapshots` keys for Nansen extended HTTP (`docs/QTSS_CURSOR_DEV_GUIDE.md` §4 ADIM 2–3).
pub const NANSEN_NETFLOWS_DATA_KEY: &str = "nansen_netflows";
pub const NANSEN_PERP_TRADES_DATA_KEY: &str = "nansen_perp_trades";
pub const NANSEN_FLOW_INTELLIGENCE_DATA_KEY: &str = "nansen_flow_intelligence";
/// Alias for dev-guide naming (`NANSEN_FLOW_INTEL_DATA_KEY`).
pub const NANSEN_FLOW_INTEL_DATA_KEY: &str = NANSEN_FLOW_INTELLIGENCE_DATA_KEY;
pub const NANSEN_WHO_BOUGHT_SOLD_DATA_KEY: &str = "nansen_who_bought_sold";
/// Alias for dev-guide naming (`NANSEN_WHO_BOUGHT_DATA_KEY`).
pub const NANSEN_WHO_BOUGHT_DATA_KEY: &str = NANSEN_WHO_BOUGHT_SOLD_DATA_KEY;
pub const NANSEN_HOLDINGS_DATA_KEY: &str = "nansen_holdings";
/// TGM perp PnL leaderboard snapshot (`nansen_extended`); feeds `nansen_whale_watchlist`.
pub const NANSEN_PERP_LEADERBOARD_DATA_KEY: &str = "nansen_perp_leaderboard";
/// Merged profiler perp-positions across whale watchlist → on-chain `hl_whale_score`.
pub const NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY: &str = "nansen_whale_perp_aggregate";
/// `app_config` row (`nansen_whale_watchlist`); JSON from perp leaderboard loop.
pub const NANSEN_WHALE_WATCHLIST_KEY: &str = "nansen_whale_watchlist";

/// TGM token flows (`POST /api/v1/tgm/flows`); opt-in loop, `data_snapshots` key.
pub const NANSEN_TGM_FLOWS_DATA_KEY: &str = "nansen_tgm_flows";
/// TGM perp trade history for a symbol (`POST /api/v1/tgm/perp-trades`); distinct from smart-money perp trades.
pub const NANSEN_TGM_PERP_TRADES_DATA_KEY: &str = "nansen_tgm_perp_trades";
pub const NANSEN_TGM_DEX_TRADES_DATA_KEY: &str = "nansen_tgm_dex_trades";
pub const NANSEN_TGM_TOKEN_INFORMATION_DATA_KEY: &str = "nansen_tgm_token_information";
pub const NANSEN_TGM_INDICATORS_DATA_KEY: &str = "nansen_tgm_indicators";
pub const NANSEN_TGM_PERP_POSITIONS_DATA_KEY: &str = "nansen_tgm_perp_positions";
pub const NANSEN_TGM_HOLDERS_DATA_KEY: &str = "nansen_tgm_holders";
/// `POST /api/v1/perp-screener` (Hyperliquid screener).
pub const NANSEN_PERP_SCREENER_DATA_KEY: &str = "nansen_perp_screener";
pub const NANSEN_SMART_MONEY_DEX_TRADES_DATA_KEY: &str = "nansen_smart_money_dex_trades";

/// Keys the copy-trade follower treats as one freshness bundle (`max_latency_ms`).
pub const REGISTERED_NANSEN_HTTP_KEYS_COPY_LATENCY: &[&str] = &[
    NANSEN_TOKEN_SCREENER_DATA_KEY,
    NANSEN_NETFLOWS_DATA_KEY,
    NANSEN_HOLDINGS_DATA_KEY,
    NANSEN_PERP_TRADES_DATA_KEY,
    NANSEN_FLOW_INTELLIGENCE_DATA_KEY,
    NANSEN_WHO_BOUGHT_SOLD_DATA_KEY,
    NANSEN_PERP_LEADERBOARD_DATA_KEY,
    NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY,
];

/// Full inventory of worker-written Nansen `data_snapshots` keys (ops / logging).
pub const REGISTERED_NANSEN_HTTP_KEYS: &[&str] = &[
    NANSEN_TOKEN_SCREENER_DATA_KEY,
    NANSEN_NETFLOWS_DATA_KEY,
    NANSEN_HOLDINGS_DATA_KEY,
    NANSEN_PERP_TRADES_DATA_KEY,
    NANSEN_FLOW_INTELLIGENCE_DATA_KEY,
    NANSEN_WHO_BOUGHT_SOLD_DATA_KEY,
    NANSEN_PERP_LEADERBOARD_DATA_KEY,
    NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY,
    NANSEN_TGM_FLOWS_DATA_KEY,
    NANSEN_TGM_PERP_TRADES_DATA_KEY,
    NANSEN_TGM_DEX_TRADES_DATA_KEY,
    NANSEN_TGM_TOKEN_INFORMATION_DATA_KEY,
    NANSEN_TGM_INDICATORS_DATA_KEY,
    NANSEN_TGM_PERP_POSITIONS_DATA_KEY,
    NANSEN_TGM_HOLDERS_DATA_KEY,
    NANSEN_PERP_SCREENER_DATA_KEY,
    NANSEN_SMART_MONEY_DEX_TRADES_DATA_KEY,
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
        description: "`nansen_extended` → `data_snapshots` smart-money/netflow (source_key=nansen_netflows)",
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
        source_key: NANSEN_PERP_LEADERBOARD_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "`nansen_extended` → `data_snapshots` tgm/perp-pnl-leaderboard + `app_config` whale watchlist",
    },
    RegisteredDataSource {
        source_key: NANSEN_WHALE_PERP_AGGREGATE_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "`nansen_extended` whale watchlist → merged profiler/perp-positions",
    },
    RegisteredDataSource {
        source_key: NANSEN_TGM_FLOWS_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "opt-in `tgm/flows`; env body or `app_config` `nansen_tgm_flows_by_symbol`",
    },
    RegisteredDataSource {
        source_key: NANSEN_TGM_PERP_TRADES_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "opt-in `tgm/perp-trades`; env or `nansen_tgm_perp_trades_by_symbol`",
    },
    RegisteredDataSource {
        source_key: NANSEN_TGM_DEX_TRADES_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "opt-in `tgm/dex-trades`; env or `nansen_tgm_dex_trades_by_symbol`",
    },
    RegisteredDataSource {
        source_key: NANSEN_TGM_TOKEN_INFORMATION_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "opt-in `tgm/token-information`; env or `nansen_tgm_token_information_by_symbol`",
    },
    RegisteredDataSource {
        source_key: NANSEN_TGM_INDICATORS_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "opt-in `tgm/indicators` (5 credits); env or `nansen_tgm_indicators_by_symbol`",
    },
    RegisteredDataSource {
        source_key: NANSEN_TGM_PERP_POSITIONS_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "opt-in `tgm/perp-positions`; env or `nansen_tgm_perp_positions_by_symbol`",
    },
    RegisteredDataSource {
        source_key: NANSEN_TGM_HOLDERS_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "opt-in `tgm/holders`; env or `nansen_tgm_holders_by_symbol`",
    },
    RegisteredDataSource {
        source_key: NANSEN_PERP_SCREENER_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "opt-in `perp-screener`; `NANSEN_PERP_SCREENER_BODY_JSON` or default date window",
    },
    RegisteredDataSource {
        source_key: NANSEN_SMART_MONEY_DEX_TRADES_DATA_KEY,
        provider_kind: "NansenHttpLoop",
        description: "opt-in `smart-money/dex-trades`; `NANSEN_SM_DEX_TRADES_BODY_JSON` or default `chains`",
    },
    RegisteredDataSource {
        source_key: "*",
        provider_kind: "HttpGenericProvider",
        description: "Her `external_data_sources` satırı; GET/POST; meta: http_status, qtss_fetch_duration_ms",
    },
];
