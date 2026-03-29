# QTSS — Cursor Geliştirme Rehberi

> **Bu doküman ne için?**
> Cursor'ın QTSS codebase'ini anlayıp doğru genişletmeler yapabilmesi için hazırlanmıştır.
> Mevcut kodun tam anatomisi, tespit edilen hatalar/eksiklikler, ve sıralı geliştirme planı içerir.
> Her bölümün başında "Cursor için özet" kutusu var — önce onu oku, detay gerekirse devam et.

---

## 0. Proje özeti

QTSS, Rust workspace tabanlı çok borsa alım-satım ve analiz platformudur.

- **Runtime:** Tokio async, tüm loop'lar `tokio::spawn` ile paralel çalışır
- **Veritabanı:** PostgreSQL (SQLx), migration'lar otomatik uygulanır
- **API:** Axum HTTP server (`qtss-api`)
- **İş mantığı:** `qtss-worker` (arka plan loop'ları)
- **Borsa:** Binance Spot + USDT-M Futures (diğerleri adapter ile)
- **On-chain veri:** Nansen, Coinglass, Hyperliquid, Binance FAPI (ücretsiz endpoint'ler)

```
crates/
├── qtss-common          # AppMode (Live/Dry), logging
├── qtss-domain          # Domain tipleri: OrderIntent, CopyRule, InstrumentId, ...
├── qtss-storage         # PostgreSQL havuzu, tüm DB fonksiyonları
├── qtss-execution       # ExecutionGateway trait, BinanceLiveGateway, DryRunGateway
├── qtss-backtest        # Backtest motoru
├── qtss-reporting       # PDF rapor
├── qtss-binance         # Binance REST + WebSocket istemcisi
├── qtss-api             # Axum HTTP API
├── qtss-worker          # Tüm arka plan loop'ları (ana çalışma yeri)
├── qtss-chart-patterns  # Elliott Wave, ACP pattern taraması
├── qtss-nansen          # Nansen HTTP (token screener + smart-money / TGM / profiler)
├── qtss-strategy        # Strateji (signal_filter, whale_momentum, arb_funding, copy_trade, risk)
└── qtss-notify          # Bildirim kanalları (Telegram, webhook, …)
```

**Migrasyonlar:** `migrations/*.sql` — geliştirme makinesi ile üretim/CI aynı dosya setini kullanmalı (`ls migrations | wc -l` uyumsuzsa repo senkronu eksik). Özet şema notları: `docs/PROJECT.md` §7; sürüm numarası ve çift önek yasağı: **§6** (bu belge).

---

## 1. Mevcut veri akışı — tam harita

```
Dış API'ler
    │
    ├─ Nansen API ──────────► nansen_engine.rs
    │                              │
    │                              ▼
    │                         nansen_snapshots (DB)
    │                         data_snapshots (DB, key="nansen_token_screener")
    │
    ├─ Binance FAPI ────────► engines/external_binance.rs
    ├─ Coinglass ───────────► engines/external_coinglass.rs   ─► data_snapshots (DB)
    ├─ Hyperliquid ─────────► engines/external_hyperliquid.rs
    └─ DeFi Llama ──────────► engines/external_misc.rs
    │
    ▼
signal_scorer.rs
    │   score_for_source_key(source_key, &json) → f64
    │   Her source_key için ayrı score_fn dispatch
    │
    ▼
onchain_signal_scorer.rs
    │   Her engine_symbol için compute_and_persist_for_symbol()
    │   weighted_aggregate_spec(parts) → (f64, f64)
    │   conflict_detected: TA signal_dashboard vs on-chain aggregate
    │
    ▼
onchain_signal_scores (DB)
    │   aggregate_score, confidence, direction, conflict_detected
    │
    ▼
API: GET /api/v1/analysis/onchain-signals/*
```

---

## 2. Kritik dosyalar ve sorumlulukları

### 2.1 qtss-worker/src/main.rs

Tüm `tokio::spawn` çağrılarının başladığı yer. Yeni bir loop eklendiğinde buraya da spawn eklenmeli.

```rust
// Mevcut spawn'lar (sırasıyla, `main.rs` — DATABASE_URL varken):
tokio::spawn(pnl_rollup_loop(pool.clone()));
tokio::spawn(engine_analysis::engine_analysis_loop(pool.clone())); // içinde confluence::compute_and_persist
tokio::spawn(nansen_engine::nansen_token_screener_loop(pool.clone()));
tokio::spawn(nansen_engine::nansen_netflows_loop(pool.clone()));
tokio::spawn(nansen_engine::nansen_holdings_loop(pool.clone()));
tokio::spawn(nansen_engine::nansen_perp_trades_loop(pool.clone()));
tokio::spawn(nansen_engine::nansen_who_bought_loop(pool.clone()));
tokio::spawn(nansen_engine::nansen_flow_intel_loop(pool.clone()));
tokio::spawn(nansen_engine::nansen_perp_leaderboard_loop(pool.clone()));
tokio::spawn(nansen_engine::nansen_whale_perp_aggregate_loop(pool.clone()));
tokio::spawn(setup_scan_engine::nansen_setup_scan_loop(pool.clone()));
tokio::spawn(engines::external_binance_loop(pool.clone()));
tokio::spawn(engines::external_coinglass_loop(pool.clone()));
tokio::spawn(engines::external_hyperliquid_loop(pool.clone()));
tokio::spawn(engines::external_misc_loop(pool.clone()));
tokio::spawn(onchain_signal_scorer::onchain_signal_loop(pool.clone()));
tokio::spawn(paper_fill_notify::paper_position_notify_loop(pool.clone()));
tokio::spawn(live_position_notify::live_position_notify_loop(pool.clone()));
tokio::spawn(kill_switch::kill_switch_loop(pool.clone()));
tokio::spawn(position_manager::position_manager_loop(pool.clone()));
tokio::spawn(copy_trade_follower::copy_trade_follower_loop(pool.clone()));
strategy_runner::spawn_if_enabled(&pool);
// Kline: QTSS_KLINE_SYMBOLS → multi_kline_ws_loop; yoksa QTSS_KLINE_SYMBOL → kline_ws_loop
```

`run_migrations` hata ayrıntısı: `crates/qtss-storage/src/pool.rs` (checksum, çift önek, `bar_intervals` eksikliği); worker `main.rs` bağlam satırı ve **§6**.

### 2.2 qtss-nansen/src/lib.rs

Nansen HTTP istemcisi. `post_json_path` + `apiKey` header. Mevcut çağrılar: `post_token_screener`, `post_smart_money_netflows` (**yol:** `api/v1/smart-money/netflow` — çoğul `/netflows` birçok planda **404**), `post_smart_money_holdings`, `post_smart_money_perp_trades`, `post_tgm_flow_intelligence`, `post_tgm_who_bought_sold`, `post_profiler_perp_leaderboard` (göreli yol parametresi; varsayılan worker env: `NANSEN_PERP_LEADERBOARD_PATH`), `post_profiler_perp_positions`.

İstek gövdeleri: `smart-money/netflow` ve `holdings` için **`chains`** dizisi; `tgm/who-bought-sold` için **`chain`** — bkz. `nansen_extended.rs` ve kök `.env.example` (`NANSEN_SMART_MONEY_CHAINS_JSON`, `NANSEN_WHO_BOUGHT_CHAIN`).

### 2.3 qtss-worker/src/signal_scorer.rs

`score_for_source_key(source_key, response) -> f64` dispatch fonksiyonu. Yeni Nansen endpoint eklendiğinde buraya da `if source_key == "nansen_netflow" { ... }` eklenmeli.

`ComponentWeights` struct'ı `app_config` tablosundan dinamik yükleniyor — yeni alan eklenince migration da gerekiyor.

### 2.4 qtss-worker/src/onchain_signal_scorer.rs

Her `engine_symbol` için çalışan ana skor hesaplama döngüsü. `compute_and_persist_for_symbol()` içinde:
1. Her `source_key` için `data_snapshots`'tan snapshot çekiliyor
2. `score_fn` çağrılıyor
3. Sonuçlar `OnchainSignalScoreInsert`'e doldurulup DB'ye yazılıyor

Yeni Nansen bileşeni eklenince bu fonksiyon içine yeni bir `snapshot_score_with_conf()` çağrısı eklenmeli.

### 2.5 qtss-storage/src/onchain_signal_scores.rs

`OnchainSignalScoreInsert` struct'ı DB kolonlarını yansıtıyor. Yeni skor kolonu eklenince hem bu struct'a hem migration'a hem de INSERT sorgusuna eklenmeli.

### 2.6 qtss-execution/src/gateway.rs

```rust
#[async_trait]
pub trait ExecutionGateway: Send + Sync {
    fn set_reference_price(&self, instrument: &InstrumentId, price: Decimal) -> Result<(), ExecutionError>;
    async fn place(&self, intent: OrderIntent) -> Result<Uuid, ExecutionError>;
    async fn cancel(&self, client_order_id: Uuid) -> Result<(), ExecutionError>;
}
```

Tüm strateji kodu bu trait üzerinden çalışır. `BinanceLiveGateway` (canlı) ve `DryRunGateway` (paper) mevcut implementasyonlar. `set_reference_price` dry/paper’da market dolum fiyatı için kullanılır.

---

## 3. Tespit edilen hatalar ve eksiklikler

> ⚠️ Bu bölüm önemli — geliştirme başlamadan önce oku.

### 3.1 ~~HATA: `nansen_sm_score` tek bir skora sıkıştırılmış~~ (giderildi)

**Durum:** `nansen_sm_score` kolonu artık yalnızca DEX baskı skoru (`score_nansen_dex_buy_sell_pressure`); ayrı kolonlarda `nansen_netflow_score`, `nansen_perp_score`, `nansen_buyer_quality_score`. Eski `depth`+`dex` birleşimi kaldırıldı; meta’da `nansen_note` ile belgelenir.

**Arşiv — eski sorun:** Token screener satır sayısı (`depth`) gerçek bir sinyal değildi; `score_nansen_response_coverage` ayrı kullanım için `signal_scorer.rs` içinde kalır, `nansen_sm_score` ile karıştırılmaz.

### 3.2 ~~HATA: `coinglass_netflow` ve `tgm/flow-intelligence` çakışması~~ (giderildi)

**Durum:** `onchain_signal_scorer.rs` içinde Nansen flow-intelligence aktif ve geçerliyken Coinglass netflow bileşen ağırlığı etkin olarak **yarıya** indirilir (`coinglass_netflow_effective`); `meta_json` içinde izlenebilir.

### 3.3 ~~EKSİKLİK: Çoklu sembol WebSocket stream~~ (giderildi)

**Durum:** `QTSS_KLINE_SYMBOLS` → `multi_kline_ws_loop` + combined URL (`qtss-binance`); yoksa `QTSS_KLINE_SYMBOL` tekil akış. Bkz. `main.rs`.

### 3.4 ~~EKSİKLİK: Copy trade yürütme~~ (giderildi)

**Durum:** `copy_trade_follower_loop` + `qtss-strategy::copy_trade` (dry runner). Abonelikler `copy_subscriptions` / `CopyRule`; env ile otomatik emir opsiyonel.

### 3.5 ~~EKSİKLİK: Otomatik SL/TP yönetimi~~ (kısmen giderildi)

**Durum:** `position_manager_loop` mevcut (`QTSS_POSITION_MANAGER_ENABLED`). Canlı bildirim ayrıca `live_position_notify.rs`. Tam otomasyon kapsamı env ve gateway moduna bağlıdır.

### 3.6 ~~EKSİKLİK: Kill switch / drawdown koruması~~ (giderildi)

**Durum:** `kill_switch_loop` + `qtss_common::halt_trading` / `is_trading_halted`; `QTSS_KILL_SWITCH_ENABLED`, `QTSS_MAX_DRAWDOWN_PCT` (referans equity ile günlük realized eşiği). **Not:** Varsayılan tasarım açık pozisyonları otomatik market kapatmaz; stratejiler `is_trading_halted` ile yeni emri durdurur. Acil kapatma `position_manager` / manuel.

### 3.7 ~~EKSİKLİK: Funding rate arbitraj stratejisi~~ (giderildi)

**Durum:** `qtss-strategy::arb_funding` + dry gateway; eşik env (`QTSS_ARB_FUNDING_THRESHOLD` vb.). Bkz. ADIM 7 / `.env.example`.

### 3.8 ~~EKSİKLİK: Nansen perp leaderboard → watchlist pipeline~~ (giderildi)

**Durum:** `nansen_perp_leaderboard_loop` → `app_config` (`NANSEN_WHALE_WATCHLIST_KEY`); `nansen_whale_perp_aggregate_loop` → `profiler/perp-positions` birleşik yanıt → `nansen_whale_perp_aggregate` snapshot → skorlama `hl_whale_score`. Leaderboard yolu: `NANSEN_PERP_LEADERBOARD_PATH` (varsayılan Hyperliquid PnL leaderboard).

### 3.9 ~~KOD KALİTESİ: `score_nansen_smart_money_depth`~~ (giderildi)

**Durum:** `signal_scorer.rs` içinde `score_nansen_response_coverage`; eski yanıltıcı isim kaldırıldı.

### 3.10 ~~KOD KALİTESİ: `confluence.rs` boş iskelet~~ (giderildi)

`qtss-worker/src/confluence.rs` rejim ağırlıkları + `data_snapshots` ile bileşik skor üretir; `insert_market_confluence_snapshot` + `upsert_analysis_snapshot` (`engine_kind = confluence`) ile kalıcı yazım yapılır.

---

## 4. Geliştirme planı — sıralı adımlar

Her adım bir öncekinden bağımsız olarak çalışabilir. Öncelik sırasına göre listelenmiştir.

---

### ADIM 1: qtss-nansen crate genişlemesi

**Dosya:** `crates/qtss-nansen/src/lib.rs`

Mevcut `post_token_screener()` pattern'ını 5 yeni fonksiyona klonla:

```rust
// Eklenecek fonksiyonlar (mevcut post_token_screener ile aynı HTTP pattern):

pub async fn post_smart_money_netflows(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,          // {"chains": [...], "pagination": {...}}
) -> Result<(Value, NansenResponseMeta), NansenError>

pub async fn post_smart_money_holdings(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError>

pub async fn post_smart_money_perp_trades(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError>

pub async fn post_tgm_flow_intelligence(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,          // {"token_address": "...", "chain": "ethereum"}
) -> Result<(Value, NansenResponseMeta), NansenError>

pub async fn post_tgm_who_bought_sold(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError>
```

**URL'ler:**
- `/api/v1/smart-money/netflow` (production’da çoğul `/netflows` sık 404 verir; `source_key` yine `nansen_netflows`)
- `/api/v1/smart-money/holdings`
- `/api/v1/smart-money/perp-trades`
- `/api/v1/tgm/flow-intelligence`
- `/api/v1/tgm/who-bought-sold`

**Header:** `apiKey` (mevcut pattern ile aynı — `apiKey` header adı küçük/büyük harfe dikkat, Nansen bunu case-sensitive kullanıyor)

---

### ADIM 2: data_sources registry güncellemesi

**Dosya:** `crates/qtss-worker/src/data_sources/registry.rs`

```rust
pub const NANSEN_NETFLOWS_DATA_KEY: &str = "nansen_netflows";
pub const NANSEN_HOLDINGS_DATA_KEY: &str = "nansen_holdings";
pub const NANSEN_PERP_TRADES_DATA_KEY: &str = "nansen_perp_trades";
pub const NANSEN_FLOW_INTEL_DATA_KEY: &str = "nansen_flow_intelligence";
pub const NANSEN_WHO_BOUGHT_DATA_KEY: &str = "nansen_who_bought_sold";
pub const NANSEN_WHALE_WATCHLIST_KEY: &str = "nansen_whale_watchlist";  // profiler/perp-positions sonuçları

// REGISTERED_DATA_SOURCES dizisine de ekle
```

---

### ADIM 3: Nansen genişletilmiş HTTP döngüleri

**Dosyalar:** `crates/qtss-worker/src/nansen_extended.rs` (loop gövdeleri + istek JSON), `nansen_engine.rs` (token screener + bu loop’ların **re-export**’u). `main.rs` spawn’ları `nansen_engine::nansen_*_loop` üzerinden.

Mevcut `nansen_token_screener_loop` ayrı kalır (`DataSourceProvider`). Diğer endpoint’ler için örnek şablon:

```rust
pub async fn nansen_netflows_loop(pool: PgPool) {
    // NANSEN_NETFLOWS_TICK_SECS env (varsayılan: 1800, min: 900)
    // data_snapshots'a key="nansen_netflows" ile yazar
    // Kredi takibi: mevcut nansen_engine.rs pattern ile aynı
}

pub async fn nansen_perp_trades_loop(pool: PgPool) {
    // NANSEN_PERP_TRADES_TICK_SECS env (varsayılan: 1800, min: 900)
    // data_snapshots'a key="nansen_perp_trades" ile yazar
    // Futures bot için kritik — öncelikli implement et
}

pub async fn nansen_flow_intel_loop(pool: PgPool) {
    // NANSEN_FLOW_INTEL_TICK_SECS env (varsayılan: 900, min: 600)
    // Her engine_symbol için ayrı çağrı — token_address gerekiyor
    // Dikkat: Bu loop engine_symbols tablosundan sembol listesi okumalı
}
```

**main.rs:** Tüm Nansen extended spawn’ları zaten tanımlı (netflows, holdings, perp_trades, who_bought, flow_intel, perp_leaderboard, whale_aggregate).

---

### ADIM 4: signal_scorer.rs genişlemesi

**Dosya:** `crates/qtss-worker/src/signal_scorer.rs`

```rust
// Eklenecek fonksiyonlar:

/// smart-money/netflows: net_buy_volume - net_sell_volume → [-1, 1]
pub fn score_nansen_netflows(response: &Value) -> f64 {
    // response.data[].net_flow alanlarını topla
    // Pozitif toplam → bullish, negatif → bearish
    // Normalize: total.abs() ile böl, clamp(-1, 1)
}

/// smart-money/perp-trades: long pozisyon ağırlığı → yön sinyali
pub fn score_nansen_perp_direction(response: &Value) -> f64 {
    // response.data[].side ("long"/"short") + notional_usd
    // long_notional / (long_notional + short_notional) → merkez 0.5'ten sapma
    // >0.6 long ağırlık → +0.4..+0.8 (smart money long)
    // <0.4 short ağırlık → -0.4..-0.8 (smart money short)
}

/// tgm/flow-intelligence: exchange net flow (Nansen versiyonu)
pub fn score_nansen_flow_intelligence(response: &Value) -> f64 {
    // response.data.net_flow veya inflow/outflow alanları
    // Coinglass netflow ile aynı mantık ama Nansen kaynaklı
    // Double-counting önlemi: bu fonksiyon coinglass_netflow ile AYNI anda aktif olmamalı
}

/// tgm/who-bought-sold: alıcı kalitesi skoru
pub fn score_nansen_buyer_quality(response: &Value) -> f64 {
    // response.data[].wallet_label alanlarını say
    // "smart_money", "fund", "vc" etiketli alıcılar → pozitif
    // "bot", "mev", "wash_trader" etiketli → negatif veya veriyi filtrele
    // Oran: (kaliteli_alıcı / toplam_alıcı) * 2 - 1 → [-1, 1]
}

// score_for_source_key dispatch'ine ekle:
if source_key == "nansen_netflows" {
    return score_nansen_netflows(response);
}
if source_key == "nansen_perp_trades" {
    return score_nansen_perp_direction(response);
}
if source_key == "nansen_flow_intelligence" {
    return score_nansen_flow_intelligence(response);
}
if source_key == "nansen_who_bought_sold" {
    return score_nansen_buyer_quality(response);
}
```

**ComponentWeights struct güncellemesi:**

```rust
#[derive(Debug, Clone, Copy)]
struct ComponentWeights {
    // Mevcut alanlar (değişmez):
    taker: f64,
    funding: f64,
    oi: f64,
    ls_ratio: f64,
    coinglass_netflow: f64,
    coinglass_liquidations: f64,
    hl_meta: f64,
    nansen: f64,
    // Yeni alanlar:
    nansen_netflows: f64,      // varsayılan: 1.0
    nansen_perp: f64,          // varsayılan: 1.5 (futures için daha kritik)
    nansen_buyer_quality: f64, // varsayılan: 0.8
}
```

---

### ADIM 5: onchain_signal_scorer.rs güncellemesi

**Dosya:** `crates/qtss-worker/src/onchain_signal_scorer.rs`

`compute_and_persist_for_symbol()` fonksiyonuna yeni bileşenler ekle:

```rust
// Mevcut nansen çağrısının ALTINA ekle:

let (nansen_nf_v, nansen_nf_c, nansen_nf_ok) =
    snapshot_score_with_conf(&pool, NANSEN_NETFLOWS_DATA_KEY, 1800, score_nansen_netflows)
        .await;

let (nansen_pt_v, nansen_pt_c, nansen_pt_ok) =
    snapshot_score_with_conf(&pool, NANSEN_PERP_TRADES_DATA_KEY, 1800, score_nansen_perp_direction)
        .await;

// parts vec'ine ekle:
if let Some(s) = nansen_nf_v {
    parts.push((s, nansen_nf_c, w.nansen_netflows));
}
if let Some(s) = nansen_pt_v {
    parts.push((s, nansen_pt_c, w.nansen_perp));
}

// OnchainSignalScoreInsert'e ekle (yeni kolonlar migration gerektirir):
// nansen_netflow_score: nansen_nf_v,
// nansen_perp_score: nansen_pt_v,
```

---

### ADIM 6: Migration ekle

**Tam zincir** (tipik sunucu): `0013`… worker / analiz, `0029`–`0033` confluence + on-chain + Nansen genişletme + ağırlıklar, **`0034_engine_symbols_fk_columns.sql`** / **`0035_engine_symbols_bar_interval_fk_if_ready.sql`** → `engine_symbols` katalog FK’ları (`bar_interval_id` yalnızca `bar_intervals` varsa 0034’te; 0035 telafi). Sürüm ↔ dosya eşlemesi ve **çift `0013`/`0014` önek hatası** için **§6**’ya bak.

**Arşiv örnek** (yalnızca `ALTER` + `app_config` fikri; ayrı dosya adı başka dallarda `0032_*.sql` olabilir):

```sql
-- Nansen genişletilmiş skor kolonları
ALTER TABLE onchain_signal_scores
    ADD COLUMN IF NOT EXISTS nansen_netflow_score DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS nansen_perp_score    DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS nansen_buyer_quality_score DOUBLE PRECISION;

-- app_config: yeni ağırlıklar (mevcut satırı güncelle)
UPDATE app_config
SET value = value ||
    '{"nansen_netflows": 1.0, "nansen_perp": 1.5, "nansen_buyer_quality": 0.8}'::jsonb
WHERE key = 'onchain_signal_weights';

-- Nansen watchlist config başlangıç değeri
INSERT INTO app_config (key, value, description)
VALUES (
    'nansen_whale_watchlist',
    '{"wallets": [], "last_updated": null}'::jsonb,
    'perp-leaderboard ile doldurulan whale cüzdan listesi'
)
ON CONFLICT (key) DO NOTHING;
```

`app_config` güncellemeleri yönetim/seed ile ayrı uygulanabilir. Yeni değişiklikler için bir sonraki boş numara (ör. `0036_*.sql`; mevcut son dosya için `ls migrations/*.sql | sort | tail`).

---

### ADIM 7: qtss-strategy crate (workspace’te mevcut)

**Klasör:** `crates/qtss-strategy/` — `conflict_policy` modülü rehberdeki `todo!` iskeletinin ötesinde; `strategy_runner` dry döngüleri env ile açılır.

**Cargo.toml eklentisi (workspace):** (depoda zaten varsa yalnızca referans; yeni klon kontrolü.)
```toml
# Cargo.toml (workspace) members listesine:
"crates/qtss-strategy",

# workspace.dependencies'e:
qtss-strategy = { path = "crates/qtss-strategy" }
```

**crates/qtss-strategy/Cargo.toml:**
```toml
[package]
name = "qtss-strategy"
edition.workspace = true

[dependencies]
tokio = { workspace = true }
tracing = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
chrono = { workspace = true }
uuid = { workspace = true }
anyhow = { workspace = true }
rust_decimal = "1"
async-trait = { workspace = true }
sqlx = { workspace = true }
qtss-common.workspace = true
qtss-domain.workspace = true
qtss-storage.workspace = true
qtss-execution.workspace = true
```

**crates/qtss-strategy/src/lib.rs:**
```rust
pub mod copy_trade;
pub mod signal_filter;
pub mod whale_momentum;
pub mod arb_funding;
pub mod risk;
pub mod context;
```

**crates/qtss-strategy/src/context.rs:** `MarketContext::load(pool, symbol, bar_exchange, bar_segment, bar_interval)` — `fetch_latest_onchain_signal_score` + `list_recent_bars` (son kapanış). Alanlar: `aggregate_score`, `confidence`, `direction`, `conflict_detected`, `market_regime`, `funding_score`, `nansen_perp_score`, `nansen_netflow_score`, `last_close`.

**crates/qtss-strategy/src/risk.rs:**
```rust
use rust_decimal::Decimal;

/// Kelly criterion ile pozisyon büyüklüğü hesapla.
/// win_rate: tarihsel kazanma oranı (0.0-1.0)
/// avg_win_loss_ratio: ortalama kazanç / ortalama kayıp
/// max_fraction: Kelly'nin üst sınırı (genellikle 0.25 — "quarter Kelly")
pub fn kelly_position_fraction(
    win_rate: f64,
    avg_win_loss_ratio: f64,
    max_fraction: f64,
) -> f64 {
    let kelly = win_rate - (1.0 - win_rate) / avg_win_loss_ratio;
    kelly.clamp(0.0, max_fraction)
}

/// Drawdown koruyucu: günlük kayıp limiti aşıldıysa sıfır döner.
pub struct DrawdownGuard {
    pub max_daily_loss_pct: f64,
    pub current_daily_pnl_pct: f64,
}

impl DrawdownGuard {
    pub fn allows_new_position(&self) -> bool {
        self.current_daily_pnl_pct > -self.max_daily_loss_pct
    }

    /// QTSS_MAX_DRAWDOWN_PCT env'den yükle (varsayılan: 5.0)
    pub fn from_env(current_daily_pnl_pct: f64) -> Self {
        let max = std::env::var("QTSS_MAX_DRAWDOWN_PCT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5.0_f64);
        Self { max_daily_loss_pct: max, current_daily_pnl_pct }
    }
}
```

**crates/qtss-strategy/src/signal_filter.rs — ana strateji:**
```rust
//! Strateji 1: Sinyal filtreli teknik analiz + Nansen doğrulama
//! 
//! Mantık:
//! 1. onchain_signal_scores'dan aggregate_score ve direction oku
//! 2. conflict_detected=true veya confidence<0.4 ise → pozisyon açma
//! 3. aggregate_score > LONG_THRESHOLD ise → long
//! 4. aggregate_score < SHORT_THRESHOLD ise → short
//! 5. Risk modülünden pozisyon büyüklüğü hesapla
//! 6. ExecutionGateway::place() ile emri ver
//! 7. Stop-loss ve take-profit emirlerini aynı anda ver

use std::sync::Arc;
use std::time::Duration;
use sqlx::PgPool;
use tracing::{info, warn};
use qtss_execution::gateway::ExecutionGateway;
use crate::context::MarketContext;
use crate::risk::DrawdownGuard;

pub struct SignalFilterStrategy {
    pub pool: PgPool,
    pub gateway: Arc<dyn ExecutionGateway>,
    pub long_threshold: f64,    // varsayılan: 0.6
    pub short_threshold: f64,   // varsayılan: -0.6
    pub min_confidence: f64,    // varsayılan: 0.4
}

impl SignalFilterStrategy {
    pub async fn run(self) {
        let tick = Duration::from_secs(
            std::env::var("QTSS_SIGNAL_FILTER_TICK_SECS")
                .ok().and_then(|s| s.parse().ok()).unwrap_or(60)
        );
        loop {
            tokio::time::sleep(tick).await;
            if let Err(e) = self.tick().await {
                warn!(%e, "signal_filter_strategy tick hatası");
            }
        }
    }

    async fn tick(&self) -> anyhow::Result<()> {
        // 1. Etkin engine_symbol'leri listele
        // 2. Her sembol için MarketContext::load()
        // 3. Sinyal kontrolü
        // 4. DrawdownGuard kontrolü
        // 5. Emir ver
        todo!()
    }
}

pub async fn run(pool: PgPool, gateway: Arc<dyn ExecutionGateway>) {
    let strategy = SignalFilterStrategy {
        pool,
        gateway,
        long_threshold: std::env::var("QTSS_LONG_THRESHOLD")
            .ok().and_then(|s| s.parse().ok()).unwrap_or(0.6),
        short_threshold: std::env::var("QTSS_SHORT_THRESHOLD")
            .ok().and_then(|s| s.parse().ok()).unwrap_or(-0.6),
        min_confidence: 0.4,
    };
    strategy.run().await;
}
```

**crates/qtss-strategy/src/whale_momentum.rs:**
```rust
//! Strateji 2: Whale takip / momentum
//!
//! Mantık:
//! 1. nansen_perp_score > 0.5 → smart money ağırlıklı long → uzun pozisyon aç
//! 2. Aynı yönde funding pozitif (uzun çok kalabalık) → giriş yapma (kontra risk)
//! 3. Momentum skoru: nansen_netflow * 0.6 + nansen_perp * 0.4
//! 4. Momentum eşiğini geçince market emri ile gir

pub async fn run(pool: sqlx::PgPool, gateway: std::sync::Arc<dyn qtss_execution::gateway::ExecutionGateway>) {
    todo!("whale_momentum stratejisi")
}
```

**crates/qtss-strategy/src/arb_funding.rs:**
```rust
//! Strateji 3: Funding rate arbitrajı
//!
//! Mantık:
//! 1. Binance funding rate > QTSS_ARB_FUNDING_THRESHOLD (varsayılan: 0.01% = 0.0001)
//!    → Spot AL + Futures SHORT (funding ücretini kazan)
//! 2. Funding rate < -QTSS_ARB_FUNDING_THRESHOLD
//!    → Spot SAT + Futures LONG
//! 3. Pozisyon açık tutulurken her funding periyodunda (8 saat) ücret alınır
//! 4. Fiyat farkı (basis) kapanınca her iki pozisyon kapatılır
//!
//! NOT: Bu strateji MarketSegment::Spot ve MarketSegment::Futures aynı anda kullanır.
//! ExecutionGateway her ikisini de destekliyor.

pub async fn run(pool: sqlx::PgPool, gateway: std::sync::Arc<dyn qtss_execution::gateway::ExecutionGateway>) {
    todo!("arb_funding stratejisi")
}
```

**crates/qtss-strategy/src/copy_trade.rs:**
```rust
//! Strateji 4: Smart money copy trading
//!
//! Mantık:
//! 1. nansen_perp_trades loop'undan smart money yeni pozisyon açtı mı kontrol et
//! 2. CopySubscription ve CopyRule'dan takip parametrelerini oku
//! 3. size_multiplier * leader_quantity = follower_quantity
//! 4. max_latency_ms kontrolü: Nansen verisi çok bayatsa (> max_latency_ms) atla
//! 5. max_notional kontrolü: pozisyon çok büyükse atla
//! 6. ExecutionGateway::place() ile emri ver
//!
//! NOT: Mevcut qtss-domain/copy_trade.rs tiplerini kullan (CopySubscription, CopyRule)
//! NOT: qtss-storage/copy_trade.rs'deki DB fonksiyonlarını kullan

pub async fn run(pool: sqlx::PgPool, gateway: std::sync::Arc<dyn qtss_execution::gateway::ExecutionGateway>) {
    todo!("copy_trade stratejisi — CopySubscription tablosundan aktif abonelikleri oku")
}
```

---

### ADIM 8: Çoklu sembol WebSocket

**Dosya:** `crates/qtss-worker/src/main.rs` — uygulandı: `QTSS_KLINE_SYMBOLS` tanımlıysa `multi_kline_ws_loop` + `public_*_combined_kline_url`; aksi halde `QTSS_KLINE_SYMBOL` ile tekil akış. Semboller **büyük harf** (örn. `BTCUSDT`) kabul edilir; URL üretimi crate içinde küçük harfe çevrilir.

---

### ADIM 9: Position manager loop

**Yeni dosya:** `crates/qtss-worker/src/position_manager.rs`

```rust
//! Açık pozisyonları izler; SL/TP tetiklendiyse otomatik emir verir.
//!
//! Akış:
//! 1. exchange_orders tablosundan status="filled" ve kapanmamış pozisyonları oku
//! 2. Her pozisyon için mevcut fiyatı market_bars'dan al (son kapanış)
//! 3. SL seviyesine düşüldüyse → ExecutionGateway::place(StopMarket) veya market emri
//! 4. TP seviyesine ulaşıldıysa → ExecutionGateway::place(TakeProfitMarket)
//! 5. Trailing stop için: mevcut fiyat - callback_rate hesapla, yeni stop seviyesi ayarla

pub async fn position_manager_loop(
    pool: sqlx::PgPool,
    gateway: std::sync::Arc<dyn qtss_execution::gateway::ExecutionGateway>,
) {
    let tick = std::time::Duration::from_secs(10); // 10 saniyede bir kontrol
    loop {
        tokio::time::sleep(tick).await;
        // ...
    }
}
```

---

### ADIM 10: Kill switch

**Yeni dosya:** `crates/qtss-worker/src/kill_switch.rs`

```rust
//! Drawdown limitini aşınca tüm pozisyonları kapatır.
//!
//! QTSS_MAX_DRAWDOWN_PCT=5  → günlük %5 kayıp → acil kapat
//! QTSS_KILL_SWITCH_ENABLED=1  → aktif et (varsayılan: 0, kapat)

use std::sync::atomic::{AtomicBool, Ordering};

/// Global kill switch durumu — tüm strateji loop'ları bunu kontrol etmeli.
pub static TRADING_HALTED: AtomicBool = AtomicBool::new(false);

pub fn is_halted() -> bool {
    TRADING_HALTED.load(Ordering::SeqCst)
}

pub fn halt() {
    TRADING_HALTED.store(true, Ordering::SeqCst);
    tracing::error!("KILL SWITCH AKTİF — tüm yeni emirler engellendi");
}

pub async fn kill_switch_loop(
    pool: sqlx::PgPool,
    gateway: std::sync::Arc<dyn qtss_execution::gateway::ExecutionGateway>,
) {
    // PnL rollup tablosundan günlük kayıp oranını hesapla
    // DrawdownGuard::from_env ile limit kontrol et
    // Limit aşıldıysa: halt() çağır + tüm açık pozisyonları kapat
}
```

**Her strateji loop'una eklenecek guard:**
```rust
// Her tick başında:
if kill_switch::is_halted() {
    tracing::warn!("kill switch aktif — emir verilmiyor");
    tokio::time::sleep(tick).await;
    continue;
}
```

---

## 5. Ortam değişkenleri — tam liste

Mevcut değişkenlere **yeni eklenecekler** kalın ile işaretlendi.

```bash
# === Mevcut ===
DATABASE_URL=postgres://...
NANSEN_API_KEY=your_key
NANSEN_API_BASE=https://api.nansen.ai    # opsiyonel
NANSEN_TICK_SECS=1800                    # token screener polling
NANSEN_INSUFFICIENT_CREDITS_SLEEP_SECS=3600
QTSS_KLINE_SYMBOL=BTCUSDT               # tekil sembol WS
QTSS_KLINE_INTERVAL=1m
QTSS_KLINE_SEGMENT=futures
QTSS_EXTERNAL_FETCH=1
QTSS_ONCHAIN_SIGNAL_ENGINE=1
QTSS_ONCHAIN_SIGNAL_TICK_SECS=60
QTSS_ONCHAIN_NOTIFY_THRESHOLD=0.6
QTSS_NOTIFY_ON_ONCHAIN_SIGNAL=1
QTSS_NOTIFY_ON_ONCHAIN_SIGNAL_CHANNELS=webhook

# === Yeni eklenecekler ===
# Nansen genişletilmiş endpoint'ler
NANSEN_NETFLOWS_TICK_SECS=1800
NANSEN_PERP_TRADES_TICK_SECS=1800
NANSEN_FLOW_INTEL_TICK_SECS=900
NANSEN_WHO_BOUGHT_TICK_SECS=1800

# Çoklu sembol WS (QTSS_KLINE_SYMBOL'ün yerini alır)
QTSS_KLINE_SYMBOLS=BTCUSDT,ETHUSDT,SOLUSDT,BNBUSDT,XRPUSDT

# Strateji parametreleri
QTSS_SIGNAL_FILTER_TICK_SECS=60
QTSS_LONG_THRESHOLD=0.6
QTSS_SHORT_THRESHOLD=-0.6
QTSS_ARB_FUNDING_THRESHOLD=0.0001       # 0.01% funding rate
QTSS_WHALE_MOMENTUM_TICK_SECS=120

# Risk yönetimi
QTSS_MAX_DRAWDOWN_PCT=5.0               # günlük maks kayıp yüzdesi
QTSS_KILL_SWITCH_ENABLED=0              # 1=aktif, 0=pasif
QTSS_MAX_POSITION_NOTIONAL_USDT=10000   # tek pozisyon maks USDT değeri
QTSS_DEFAULT_LEVERAGE=3                 # futures kaldıraç (1-20)
QTSS_KELLY_APPLY=0                      # 1=strateji taban miktarına Kelly çarpanı
QTSS_KELLY_WIN_RATE=0.55
QTSS_KELLY_AVG_WIN_LOSS_RATIO=1.5
QTSS_KELLY_MAX_FRACTION=0.25

# Position manager
QTSS_POSITION_MANAGER_TICK_SECS=10
QTSS_DEFAULT_STOP_LOSS_PCT=2.0          # yüzde olarak
QTSS_DEFAULT_TAKE_PROFIT_PCT=4.0
```

---

## 6. Migration numaralandırma kuralı

### 6.1 SQLx sürümü = dosya adındaki sayı

`sqlx::migrate!` dosya adının **ilk alt çizgiye kadar olan sayısal önekini** `_sqlx_migrations.version` olarak kullanır.

- `0029_market_confluence_payload_column.sql` → sürüm **29**
- `0030_onchain_signal_scores.sql` → sürüm **30**
- `0034_engine_symbols_fk_columns.sql` → sürüm **34**
- `0035_engine_symbols_bar_interval_fk_if_ready.sql` → sürüm **35**

Örnek canlı DB sırası (özet):

| version | Örnek dosya / açıklama |
|--------|-------------------------|
| 29 | `market_confluence_payload_column` |
| 30 | `onchain_signal_scores` |
| 31 | `onchain_signal_weights_app_config` |
| 32 | `nansen_extended_scores` |
| 33 | `onchain_weights_hl_whale` |
| 34 | `engine_symbols_fk_columns` — `exchange_id` vb.; `bar_interval_id` yalnızca `bar_intervals` tablosu varsa |
| 35 | `engine_symbols_bar_interval_fk_if_ready` — `bar_intervals` sonradan gelirse `bar_interval_id` telafisi |

Kontrol:

```sql
SELECT version, description FROM _sqlx_migrations ORDER BY version DESC LIMIT 10;
```

```bash
ls migrations/*.sql | sort | tail -10
```

### 6.2 Aynı önek iki kez kullanılamaz (kritik)

Aynı numaralı iki dosya SQLx’i bozar; depoda **tek** önek kalmalı:

- **0013:** `0013_worker_analytics_schema.sql` — `bar_intervals` genişletmesi (eski `0013_bar_intervals.sql`) bu dosyada birleştirildi; ayrı `0013_*.sql` yok.
- **0014:** yalnızca `0014_catalog_fk_columns.sql` (eski `0014_engine_symbols_catalog_fk_repair.sql` kaldırıldı).

`0034_engine_symbols_fk_columns.sql` eksik FK kolonları için idempotent `ADD COLUMN` sağlar (`bar_intervals` yoksa `bar_interval_id` atlanır). `0035_engine_symbols_bar_interval_fk_if_ready.sql` tablo sonradan oluşunca tamamlar. Uygulanmış migrasyon dosyasını değiştirmeyin (§8 kural 6).

### 6.3 Yeni migration

Bir sonraki boş sürümü kullanın (ör. dizinde en yüksek önek 35 ise **`0036_yeni_ozellik.sql`**). Çift `0013_*.sql` / `0014_*.sql` açmayın.

---

## 7. Test stratejisi

### 7.1 Birim testler

Mevcut `signal_scorer.rs` içinde `#[cfg(test)]` bloğu var — yeni score fonksiyonları için aynı pattern:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nansen_netflows_bullish() {
        let v = json!({
            "data": [
                {"net_flow": 1000000.0, "symbol": "ETH"},
                {"net_flow": 500000.0, "symbol": "BTC"}
            ]
        });
        let s = score_nansen_netflows(&v);
        assert!(s > 0.0, "pozitif netflow bullish olmalı");
        assert!(s <= 1.0);
    }

    #[test]
    fn nansen_perp_direction_heavy_long() {
        let v = json!({
            "data": [
                {"side": "long", "notional_usd": 8000000.0},
                {"side": "short", "notional_usd": 2000000.0}
            ]
        });
        let s = score_nansen_perp_direction(&v);
        assert!(s > 0.3, "ağırlıklı long pozisyon bullish olmalı");
    }
}
```

### 7.2 Dry run testi

Yeni strateji implemente edilince `AppMode::Dry` ile test et:
```bash
APP_MODE=dry cargo run -p qtss-worker
```

`DryRunGateway` emirleri paper ledger'a yazacak, gerçek borsa çağrısı olmayacak.

---

## 8. Kod kalitesi kuralları (proje geneli)

Mevcut kodda uygulanan kurallar — yeni dosyalarda da uygulanmalı:

1. **Türkçe yorumlar, İngilizce identifier'lar.** Yorum satırları Türkçe olabilir, değişken/fonksiyon/struct adları İngilizce `snake_case`.

2. **Her loop env'den kontrol edilebilir.** Yeni loop'lar mutlaka `QTSS_X_ENABLED=0` ile devre dışı bırakılabilmeli. Örnek: `if !engine_enabled() { return; }`

3. **Hata durumunda `warn!` yaz, panic etme.** Loop'lar `loop { ... if err { warn!(); sleep(); continue; } }` pattern'ı izler.

4. **Kredi/limit takibi.** Nansen çağrıları `NansenResponseMeta` ile kredi bilgisini loglamalı. Yetersiz kredi durumunda uzun uyku (`insufficient_sleep`) uygulanmalı.

5. **DB yazımı her zaman `upsert`.** `INSERT ... ON CONFLICT DO UPDATE` — duplicate kayıt hatası olmamalı.

6. **Migration dosyasını değiştirme.** Uygulanmış migration'lar düzenlenmez; sadece yeni numara ile yeni dosya ekle.

7. **`must_use` attribute.** Skor fonksiyonları ve hesaplama fonksiyonları `#[must_use]` ile işaretlenmeli (mevcut kodda da var).

---

## 9. Öncelik sırası özeti

| Adım | Dosya/Crate | Durum (bu repo) |
|------|-------------|-----------------|
| 1 | qtss-nansen: smart-money / TGM / profiler | Tamam |
| 2 | registry.rs: Nansen + harici source_key | Tamam |
| 3 | nansen_extended + nansen_engine re-export | Tamam |
| 4 | signal_scorer.rs: netflows, perp, flow-intel, buyer quality, … | Tamam |
| 5 | onchain_signal_scorer.rs: bileşenler + Coinglass/flow-intel yarım ağırlık | Tamam |
| 6 | migrations `0013`→`0035` (worker, on-chain, confluence, `0034`/`0035` engine_symbols FK) | Tamam; sunucu `ls migrations` ile doğrula; çift `0013`/`0014` önek yok |
| 7 | qtss-strategy + strategy_runner (dry) | Tamam |
| 8 | Çoklu sembol WS (`QTSS_KLINE_SYMBOLS`) | Tamam |
| 9 | position_manager.rs | Tamam (env ile kapalı) |
| 10 | kill_switch.rs | Tamam (env ile kapalı) |

**Sonraki işler:** yeni özellik = yeni migration numarası (§6); üretimde `DATABASE_URL` + Nansen/plan limitleri; strateji canlı gateway için ayrı onay ve risk kontrolleri.

**Geçmiş plan:** Aşağıdaki ADIM 1–10 metinleri referans içindir — uygulama yukarıdaki tabloyla hizalanmıştır.

---

## 10. Sık sorulan sorular

**S: Yeni Nansen endpoint ekleyince kaç kredi harcar?**
Her `smart-money/*` endpoint'i 5 kredi/çağrı. `tgm/*` endpoint'leri 1 kredi. 1800s tick ile günde ~48 çağrı × 5 kredi = ~240 kredi/gün/endpoint. 4 yeni endpoint = ~960 kredi/gün ek maliyet.

**S: Migration uygulandı mı kontrol nasıl yapılır?**
```sql
SELECT version, description FROM _sqlx_migrations ORDER BY version DESC LIMIT 5;
```

**S: Worker'ı yeniden başlatmadan config değiştirilebilir mi?**
`app_config` tablosundaki değerler (ağırlıklar, screener body) her döngüde DB'den okunur — restart gerekmez. Env değişkeni değişikliği ise restart gerektirir.

**S: Dry run modunda hangi işlemler gerçekleşir?**
`DryRunGateway` aktif olduğunda hiçbir gerçek emir Binance'a gitmez. Sadece `paper_fills` ve `paper_ledger` tabloları güncellenir. Veri toplama (Nansen, Coinglass, WS) dry run'da da çalışır.

**S: `conflict_detected=true` olduğunda strateji ne yapmalı?**
API tarafında bilgi dönüyor. `qtss-strategy::signal_filter` içinde `QTSS_SIGNAL_FILTER_ON_CONFLICT=half` (veya `none` / `skip`) ile çatışmada yarım miktar veya atlama uygulanır; bkz. `conflict_policy.rs`.
