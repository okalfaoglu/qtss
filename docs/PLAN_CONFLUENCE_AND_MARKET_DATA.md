# Plan — Market data ingestion, confluence, notifications (English identifiers)

This document extends the product roadmap: **multi-source market context**, **confluence scoring**, **Telegram (and other) notifications** for setups and open positions. It incorporates the **seven signal source categories** discussed in chat (Mar 28) and the **latency policy**: prefer **low-latency / exchange-direct** feeds over delayed free tiers.

**Naming rule (mandatory for new work):** All new **Rust fields**, **JSON keys**, **DB columns**, and **API payloads** use **English `snake_case`**. UI copy may stay Turkish (or any locale) via i18n layers; the wire format stays English.

**Legacy note:** `SignalDashboardV1` in `qtss-chart-patterns` still exposes Turkish string *values* (e.g. `YUKARI`) and Turkish *field names* (`durum`, `yerel_trend`, …). Migrating to English keys is a **separate phased refactor** (see §7) so existing `analysis_snapshots` / web clients can migrate safely.

---

## 1. Signal sources — what we pull and why (mapped to QTSS)

| Source category | Typical metrics | Free? | QTSS integration | Use in project |
|-----------------|-----------------|-------|------------------|----------------|
| **smart_money** | Labelled wallet flows, token screener rows | Paid (Nansen, Glassnode) | `nansen_snapshots`, `nansen_setup_*` | Early positioning, token-level long/short setups |
| **cex_flow** | Exchange net inflow/outflow, reserves | CQ free tier often **delayed** | Prefer **Coinglass** netflow / exchange balance via `external_fetch`; optional paid CQ | Bearish if heavy inflow to CEX; bullish if net outflow |
| **whale_transfers** | Large transfers, exchange tags | Whale Alert limited; on-chain indexers | Nansen row fields + optional `external_fetch` (e.g. Coinglass balance) | Short-term pressure / distribution proxy |
| **dex_pressure** | Buy vs sell notional, swap counts | The Graph / Llama (aggregate often slow) | **Nansen** `buy_volume` / `sell_volume` on screener rows; optional per-pair Graph later | Demand vs supply on DEX for screened tokens |
| **hyperliquid** | Funding, OI, mark; per-wallet via signed calls | Yes (public `info`) | `setup_scan_engine` HL enrich + `external_data_sources` row `metaAndAssetCtxs` | Perp crowding / funding tilt by coin |
| **funding_oi** | Funding rate, OI, long/short ratios | Binance FAPI + Coinglass (key) | `external_fetch` URLs + optional `qtss-binance` helpers | Leverage heat; trend health (price + OI) |
| **liquidations** | Liq clusters, recent liq volume | Coinglass (key) | `external_fetch` | Magnet levels; squeeze / flush context |

**Combined read (conceptual):** For a symbol, join **technical** (`trading_range`, `signal_dashboard`, optional Elliott/ACP) with **context** (`nansen_setup_rows` when token matches, `data_snapshots` by `source_key`, HL enrich fields in `raw_metrics`). **Confluence** = weighted agreement / conflict detection (no tight coupling between collectors).

**Diagrams you shared** (unified trait + confluence): equivalent flows are captured below as **Mermaid** so they are readable without images.

---

## 2. Latency policy (from chat — “delayed out, real-time in”)

| Avoid for *active* signals | Reason | Preferred replacement |
|----------------------------|--------|------------------------|
| CryptoQuant **free** EOD / coarse cadence | Too slow for intraday decisions | Coinglass netflow / balance endpoints + Binance taker ratio |
| Whale Alert **free** rate limits + lag | Unreliable for automation | HL `metaAndAssetCtxs` + funding/OI; optional known-wallet watchlist |
| DeFi Llama as **primary** token-level DEX pressure | Aggregate / refresh lag | Nansen screener DEX fields; The Graph per-pair when needed |

**Nansen** remains on a **conscious tick** (e.g. 30m): acceptable for smart-money *context*, not for sub-second execution.

**Operator guide (env, API, troubleshooting):** [`docs/NANSEN_TOKEN_SCREENER.md`](./NANSEN_TOKEN_SCREENER.md).

**Example `external_data_sources` rows** (placeholders — verify URLs and add `headers_json` for Coinglass):

```sql
-- Illustrative only; run after validating API paths and keys.
INSERT INTO external_data_sources (key, method, url, body_json, tick_secs, description) VALUES
('coinglass_netflow_btc', 'GET',
 'https://open-api.coinglass.com/public/v2/exchange/netflow?symbol=BTC&ex=Binance',
 NULL, 300, 'BTC Binance netflow proxy'),
('binance_taker_btcusdt', 'GET',
 'https://fapi.binance.com/futures/data/takerlongshortRatio?symbol=BTCUSDT&period=5m&limit=10',
 NULL, 60, 'BTCUSDT taker buy/sell ratio'),
('coinglass_exchange_balance_btc', 'GET',
 'https://open-api.coinglass.com/public/v2/exchange/balance?symbol=BTC',
 NULL, 300, 'BTC multi-exchange balance proxy');
-- HL whale wallet requires a real address; optional.
```

---

## 3. Unified data collection — `DataSourceProvider` trait (target architecture)

**Goal (matches your first diagram):** One **trait** for *how* data is fetched; **different** implementations for Nansen (credits, headers, errors), generic HTTP (`external_fetch`), and future GraphQL / WebSocket sources. The **scorer** dispatches on `source_key` and does not care which provider ran.

### 3.1 Trait contract (sketch — English names only)

```rust
// Implemented: `crates/qtss-worker/src/data_sources/provider.rs`
#[async_trait::async_trait]
pub trait DataSourceProvider: Send + Sync {
    /// Same as DB `source_key` (`&str` — HTTP rows use `external_data_sources.key`).
    fn source_key(&self) -> &str;

    /// Upserted to `data_snapshots` via `persist_fetch_to_data_snapshot`; HTTP status lives in `meta_json` (e.g. `http_status`).
    async fn fetch(&self) -> DataSourceFetchOk;
}

pub struct DataSourceFetchOk {
    pub request_json: serde_json::Value,
    pub response_json: Option<serde_json::Value>,
    pub meta_json: Option<serde_json::Value>,
    pub error: Option<String>,
}
```

**Implementations:**

| Type | Role | Backing code today |
|------|------|--------------------|
| `NansenTokenScreenerProvider` | Credits + `post_token_screener` | `nansen_engine.rs`, `qtss-nansen` |
| `HttpGenericProvider` | Config-driven GET/POST | `external_fetch_engine.rs`, `external_data_sources` |
| `BinanceFapiProvider` (optional) | Signed or public FAPI where needed | `qtss-binance` (future) |
| `GraphQlProvider` / `WsProvider` | Future | new impl, same trait |

**Worker registration:** a `Vec<Arc<dyn DataSourceProvider>>` or small registry; one supervisor loop calls `fetch` per provider on its schedule (reuse `tick_secs` from config / Nansen env).

### 3.2 Unified storage: `data_snapshots` for HTTP (+ `nansen_snapshots` for Nansen)

| Phase | Storage | Notes |
|-------|---------|--------|
| **Nansen** | `nansen_snapshots` | Token screener; ayrıca `data_snapshots` (`nansen_token_screener`) confluence için. |
| **Generic HTTP** | `data_snapshots` only | `external_data_snapshots` kaldırıldı (`migrations/0024_drop_external_data_snapshots.sql`). |

```mermaid
flowchart TB
  subgraph providers [DataSourceProvider implementations]
    NP[NansenProvider]
    HG[HttpGenericProvider]
    FP[Future: GraphQL / WS]
  end
  subgraph store [Storage]
    DS[(data_snapshots)]
  end
  subgraph score [Scoring]
    SC[SignalScorer: score_fn per source_key]
    AG[aggregate]
  end
  NP --> DS
  HG --> DS
  FP --> DS
  DS --> SC --> AG
```

### 3.3 Adding a new API (3 steps)

1. **Implement** `DataSourceProvider` (or add a row for `HttpGenericProvider` only).
2. **Register** the provider in the worker registry (or insert `external_data_sources`).
3. **Register** a `score_fn` for that `source_key` in `SignalScorer` (or config-driven formula id).

---

## 4. Confluence architecture (regime weights + engine_analysis)

**Flow (matches your second diagram):** Regime comes from existing TA (today: `signal_dashboard` / `piyasa_modu` — map to **English** regime codes in confluence layer: `range`, `trend`, `breakout`, `uncertain`). Weights load from **`app_config`** (JSON). Three **input groups** feed the confluence engine; outputs go to `analysis_snapshots` (`engine_kind = "confluence"`), notify, and position sizing policy.

```mermaid
flowchart TB
  REG[Regime detection from TA snapshot]
  W[Dynamic weights app_config: confluence_weights_by_regime]
  T[technical_signals: range sweep dashboard acp elliott]
  O[onchain_signals: funding_oi cex_flow liquidation hl_bias dex_whale]
  S[smart_money: nansen_screener nansen_setup]
  REG --> W
  W --> CE[Confluence engine]
  T --> CE
  O --> CE
  S --> CE
  CE --> V[conflict detection + confidence_score]
  V --> SD[signal_dashboard v2 merge optional]
  V --> NT[qtss-notify thresholds]
  V --> PS[position size multiplier]
```

### 4.1 Example `app_config` value (English keys)

Config key suggestion: `confluence_weights_by_regime`.

```json
{
  "range":    { "technical": 0.50, "onchain": 0.35, "smart_money": 0.15 },
  "trend":    { "technical": 0.30, "onchain": 0.40, "smart_money": 0.30 },
  "breakout": { "technical": 0.40, "onchain": 0.45, "smart_money": 0.15 },
  "uncertain":{ "technical": 0.20, "onchain": 0.30, "smart_money": 0.50 }
}
```

**Regime mapping:** Implement a small function `fn map_market_mode_to_regime(legacy: &str) -> &'static str` translating current Turkish mode strings to the four English keys above (until `SignalDashboardV2` emits `market_mode` in English).

### 4.2 `engine_analysis` integration

After `trading_range` + `signal_dashboard` are computed for a symbol, run **confluence**:

1. Read latest `data_snapshots` rows (or legacy dual read) for relevant `source_key`s.
2. Resolve regime → weights from `app_config`.
3. Produce `confluence` JSON: `pillar_scores`, `composite_score`, `confidence_0_100`, `conflicts: [{ "code": "ta_long_vs_funding_crowded_long", "severity": "warn" }]`.
4. `upsert_analysis_snapshot(..., engine_kind = "confluence", ...)`.

**Conflict rule (example):** TA bias long + extreme positive funding + bearish CEX flow → lower `confidence_0_100` and expose `lot_scale` suggestion (advisory until execution policy consumes it).

**Implemented (worker confluence JSON):** `schema_version` **2** adds advisory **`lot_scale_hint`** in **[0.5, 1.0]**, derived from conflict count (`1 - 0.12 * n`, clamped). **`conflicts`** entries use English **`code`** / **`severity`**; additional codes include e.g. `breakout_regime_strong_onchain_bias`, `strong_ta_thin_smart_money`, `technical_vs_weighted_composite_opposed` alongside TA vs funding / onchain pairs. Web **Motor** and **Bağlam** drawers surface `lot_scale_hint` and a short conflict summary.

---

## 5. Confluence vs Elliott / ACP / Trading Range

| Regime (from `market_mode` / TR logic) | Primary engine | Secondary | External context |
|----------------------------------------|----------------|-----------|------------------|
| **range** | `trading_range` + sweeps | ACP channel break confirmation | Funding / liq as caution |
| **breakout** (`KOPUS` equivalent) | Sweep signals | ACP | HL funding extreme → fade or size down |
| **trend** | Trend structure + optional Elliott impulse | — | OI + price agreement → healthy trend |
| **uncertain** | Lower TA weight | — | Nansen + derivatives snapshots weigh more |

**Rule:** modules **validate** each other; they should not call each other deeply. Emit **conflict flags** (e.g. `ta_direction` vs `derivatives_crowded_long`) in `confluence` payload.

---

## 6. Implementation phases (English JSON / columns)

### Phase A — Data plane (mostly config)

- Register `external_data_sources` rows (ops API or SQL): e.g. `hl_meta_asset_ctxs`, `binance_btc_taker_ratio`, `coinglass_btc_netflow` (exact URLs + `headers_json` for Coinglass API key).
- Document `source_key` naming: `snake_case`, stable, per asset if needed (`coinglass_netflow_btc`).

### Phase B — Persistence for derived scores

- New migration: e.g. `market_confluence_snapshots` or `symbol_context_scores` with columns such as:
  - `symbol` (or `instrument_id` later), `computed_at`, `schema_version`
  - `scores_json` (English keys: `smart_money`, `cex_flow`, `dex_pressure`, `hyperliquid`, `funding_oi`, `liquidations`, `composite`)
  - `conflicts_json` (e.g. `ta_vs_funding: true`)
- Worker loop: `market_confluence_loop` — reads snapshots + Nansen setup join rules, writes one row per symbol (or batch JSON).

### Phase C — API

- **`GET /api/v1/analysis/market-context/latest`** — `symbol` (required); optional `interval`, `exchange`, `segment` to pick one `engine_symbols` row. Response (English keys): `technical.signal_dashboard`, `technical.trading_range`, `confluence`, `context_data_snapshots` (Nansen `nansen_token_screener` + `binance_taker_{base}usdt` when present).
- **`GET /api/v1/analysis/market-context/summary`** — optional `exchange`, `segment`, `symbol`, `enabled_only`, `limit`: motor hedefleri + kısa TA / confluence alanları (web **Bağlam** özet tablosu).
- Daha geniş tenant / strateji / tarih filtreleri — future.
- Reuse dashboard RBAC patterns (`require_dashboard_roles`).

### Phase D — Notifications

- **Setup card (partial):** after successful `nansen_setup` run, if `QTSS_NOTIFY_SETUP_ENABLED` and any ranked row passes `QTSS_NOTIFY_SETUP_MIN_SCORE` + `QTSS_NOTIFY_SETUP_MIN_PROBABILITY` → one summary `Notification` (Turkish body; `run_id` in title). `crates/qtss-worker/src/setup_scan_engine.rs` — `maybe_notify_nansen_setup_run`.
- **Live position:** full picture needs **position state** (avg entry, side, SL, TP, optional `protection`) from `exchange_orders` / future `open_positions` + live price from `market_bars` or ticker — **worker / reconcile (sonraki adım)**.
- **MVP (dry):** `paper_fills` watcher in `qtss-worker` (`paper_fill_notify.rs`) — env `QTSS_NOTIFY_PAPER_POSITION_ENABLED`, `QTSS_NOTIFY_PAPER_POSITION_CHANNELS` (or legacy `QTSS_NOTIFY_POSITION_*`), poll `QTSS_NOTIFY_POSITION_TICK_SECS` (default 30s). Startup establishes a time baseline; only fills **after** that window are notified (restart avoids spamming history).

Env: `QTSS_NOTIFY_SETUP_*` + paper/position vars (see `.env.example`).

### Phase E — Web dashboard

- **Partial:** drawer tab **Bağlam** (`market_context`) — `market-context/latest`, **`market-context/summary`**, `engine/confluence/latest`, `data-snapshots`; `web/src/App.tsx` + `client.ts` (`fetchMarketContextLatest`, `fetchMarketContextSummary`, `fetchConfluenceSnapshotsLatest`, …).
- Motor **Range / Paper (F4)** kartında **komisyon özeti (F5 GUI)** — `fetchBinanceCommissionDefaults` (panel yenileme), `fetchBinanceCommissionAccount` (düğme; `exchange_accounts`); bkz. `SPEC_EXECUTION_RANGE_SIGNALS_UI.md` §7.1.
- Signal card: Motor sekmesinde sinyal tablosu — satır etiketleri Türkçe; değerler **`signal_dashboard_v2`** (`schema_version` 3) varsa oradan, yoksa v1’den; v2 için **Wire (EN)** `<details>` (`web/src/lib/signalDashboardPayload.ts`, `App.tsx`).

### Phase F — `SignalDashboardV2` (refactor)

- **Partial (dual-write):** `signal_dashboard` JSON içinde Türkçe v1 alanları **aynı kalır** (`schema_version: 2` v1 struct’ta); ek olarak **`signal_dashboard_v2`** nesnesi (`schema_version: 3`, İngilizce `snake_case` anahtarlar) — `crates/qtss-chart-patterns/src/dashboard_v2_envelope.rs` (`SignalDashboardV2Envelope`, `signal_dashboard_v2_envelope_from_v1`); worker `enrich_dashboard_payload` (`engine_analysis.rs`) ile yazılır.
- **Sonraki:** Diğer web / API tüketicilerini `signal_dashboard_v2`’ye geçirmek; v1 alanlarını kaldırma (deprecation) ayrı faz. (Motor GUI: Wire (EN) paneli eklendi.)

### Phase G — `DataSourceProvider` + optional `data_snapshots` migration

- **Done (partial):** `DataSourceProvider`, `DataSourceFetchOk`, `HttpGenericProvider` (`data_sources/http_generic.rs`), `persist_fetch_to_data_snapshot` (`persist.rs`), `external_fetch_loop` uses the trait; well-known keys in `registry.rs` (e.g. Nansen `data_snapshots` key).
- **Later:** `NansenTokenScreenerProvider` implementing the same trait (optional — today `nansen_engine` loop stays as-is).
- `external_data_snapshots` removed; HTTP ham yanıt yalnız `data_snapshots`.
- Point `SignalScorer` at unified read API.

---

## 7. Chat / diagram recap

- **Unified collection diagram:** §3 — `DataSourceProvider`, unified `data_snapshots` target, scorer dispatch by `source_key`.
- **Confluence diagram:** §4 — regime → `app_config` weights → three pillars → engine → `signal_dashboard` v2 / notify / position sizing.
- **Latency swap table:** §2 + SQL examples — CQ free / Whale Alert free / Llama-as-primary DEX pressure out; Coinglass + Binance taker + HL where applicable.
- **Module matrix:** §5 — TR/ACP/Elliott vs on-chain / smart money by regime.

---

## 8. Traceability

| Artifact | Location |
|----------|----------|
| Generic HTTP fetch | `migrations/0021_external_data_fetch.sql` (kaynak tablo), `migrations/0024_drop_external_data_snapshots.sql`, `migrations/0022_data_snapshots_confluence.sql`, `migrations/0023_external_data_sources_seed_f7.sql`, `crates/qtss-worker/src/external_fetch_engine.rs`, `crates/qtss-api/src/routes/external_fetch.rs` → okuma `data_snapshots` |
| Market context (merged read) | `GET /api/v1/analysis/market-context/latest` — `crates/qtss-api/src/routes/analysis.rs`; `list_engine_symbols_matching` — `crates/qtss-storage/src/engine_analysis.rs` |
| Market context (filtreli özet) | `GET /api/v1/analysis/market-context/summary` — `analysis.rs`; `list_market_context_summaries` — `engine_analysis.rs`; web **Bağlam** “Motor hedefleri (filtreli özet)” |
| Web “Bağlam” sekmesi | `web/src/App.tsx` (`market_context`), `web/src/api/client.ts` (`fetchMarketContextLatest`, `fetchMarketContextSummary`, `fetchConfluenceSnapshotsLatest`, `fetchDataSnapshots`) |
| Web Motor komisyon (F5 GUI) | `web/src/App.tsx` (F4 kartı altında), `fetchBinanceCommissionDefaults` / `fetchBinanceCommissionAccount` — `SPEC_EXECUTION_RANGE_SIGNALS_UI.md` §7.1 |
| `DataSourceProvider` (Phase G) | `crates/qtss-worker/src/data_sources/` — `provider.rs`, `http_generic.rs`, `persist.rs`, `registry.rs` |
| Nansen token screener (rehber) | [`docs/NANSEN_TOKEN_SCREENER.md`](./NANSEN_TOKEN_SCREENER.md) |
| Nansen worker + HTTP client | `crates/qtss-worker/src/nansen_engine.rs`, `crates/qtss-nansen/src/lib.rs` (`POST …/api/v1/token-screener`) |
| Nansen dynamic body | `crates/qtss-worker/src/nansen_query.rs`, `app_config` key `nansen_screener_request` (öncelik: config → `NANSEN_TOKEN_SCREENER_REQUEST_JSON` → default) |
| HL enrich in setups | `crates/qtss-worker/src/setup_scan_engine.rs` |
| Current TA dashboard | `crates/qtss-chart-patterns/src/dashboard_v1.rs`, `dashboard_v2_envelope.rs`, `engine_analysis.rs` (`signal_dashboard_v2` dual-write) |
| Notifications | `crates/qtss-notify`, `QTSS_NOTIFY_*`; setup özeti `setup_scan_engine.rs` (`QTSS_NOTIFY_SETUP_*`); dry dolum `paper_fill_notify.rs` (`QTSS_NOTIFY_PAPER_POSITION_*`, `QTSS_NOTIFY_POSITION_TICK_SECS`) |
| Execution / range spec | `docs/SPEC_EXECUTION_RANGE_SIGNALS_UI.md` (F7 row) |

---

*Plan version: 2 — adds trait + confluence Mermaid, `app_config` weights, `data_snapshots` target, Phase G.*
