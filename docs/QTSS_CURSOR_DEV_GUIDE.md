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
└── qtss-nansen          # Nansen HTTP istemcisi (şu an sadece token-screener)
```

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
// Mevcut spawn'lar (sırasıyla):
tokio::spawn(pnl_rollup_loop(pool.clone()));
tokio::spawn(engine_analysis::engine_analysis_loop(pool.clone()));
tokio::spawn(nansen_engine::nansen_token_screener_loop(pool.clone()));
tokio::spawn(setup_scan_engine::nansen_setup_scan_loop(pool.clone()));
tokio::spawn(engines::external_binance_loop(pool.clone()));
tokio::spawn(engines::external_coinglass_loop(pool.clone()));
tokio::spawn(engines::external_hyperliquid_loop(pool.clone()));
tokio::spawn(engines::external_misc_loop(pool.clone()));
tokio::spawn(onchain_signal_scorer::onchain_signal_loop(pool.clone()));
tokio::spawn(paper_fill_notify::paper_position_notify_loop(pool.clone()));
tokio::spawn(live_position_notify::live_position_notify_loop(pool.clone()));
```

### 2.2 qtss-nansen/src/lib.rs

Nansen HTTP istemcisi. Şu an **yalnızca** `post_token_screener()` fonksiyonu var. Tüm yeni Nansen endpoint'leri buraya eklenmeli. Pattern:

```rust
pub async fn post_smart_money_netflows(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: &Value,
) -> Result<(Value, NansenResponseMeta), NansenError> {
    let url = format!("{}/api/v1/smart-money/netflows", base_url.trim_end_matches('/'));
    // ... post_token_screener ile aynı HTTP pattern
}
```

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
    async fn place(&self, intent: OrderIntent) -> Result<Uuid, ExecutionError>;
    async fn cancel(&self, client_order_id: Uuid) -> Result<(), ExecutionError>;
}
```

Tüm strateji kodu bu trait üzerinden çalışır. `BinanceLiveGateway` (canlı) ve `DryRunGateway` (paper) mevcut implementasyonlar.

---

## 3. Tespit edilen hatalar ve eksiklikler

> ⚠️ Bu bölüm önemli — geliştirme başlamadan önce oku.

### 3.1 HATA: `nansen_sm_score` tek bir skora sıkıştırılmış

**Sorun:** `onchain_signal_scorer.rs` içinde Nansen skoru `depth` ve `dex` olmak üzere iki alt skoru tek `nansen_sm_score` kolonuna birleştiriyor:

```rust
// Mevcut — yanlış tasarım
let (nansen_v, ...) = snapshot_score_with_conf(..., |j| {
    let depth = score_nansen_smart_money_depth(j);
    let dex = score_nansen_dex_buy_sell_pressure(j);
    (0.55 * depth + 0.45 * dex).clamp(-1.0, 1.0)
})
```

**Sorun:** Token screener satır sayısı (`depth`) gerçek bir sinyal değil — Nansen'in kaç token döndürdüğünü ölçüyor, smart money yönünü değil.

**Çözüm:** `score_nansen_smart_money_depth()` kaldırılmalı veya ağırlığı sıfırlanmalı. Yalnızca `score_nansen_dex_buy_sell_pressure()` kullanılmalı ve adı `nansen_dex_pressure_score` olarak açıklanmalı.

### 3.2 HATA: `coinglass_netflow` ve `tgm/flow-intelligence` çakışması riski

**Sorun:** Coinglass netflow (`coinglass_netflow_btc`) ve Nansen `tgm/flow-intelligence` aynı "borsadan coin çıkışı" metriğini ölçüyor. İkisi birlikte `onchain_signal_weights`'te eşit ağırlık alırsa aynı sinyal iki kez sayılmış olur (double-counting).

**Çözüm:** `tgm/flow-intelligence` eklendiğinde `coinglass_netflow` ağırlığı `0.5`'e düşürülmeli. Ya da ikisi birleştirilip tek `exchange_netflow_score` kolonuna yazılmalı, hangisi daha taze ise o kullanılmalı.

### 3.3 EKSİKLİK: Çoklu sembol WebSocket stream

**Mevcut:** `QTSS_KLINE_SYMBOL` env ile tek sembol dinleniyor.

**Sorun:** 4 strateji ve çoklu market için onlarca sembol gerekiyor. Her biri için ayrı WS bağlantısı açmak Binance'ın bağlantı limitini aşar.

**Çözüm:** Binance Combined Stream URL kullanılmalı:
```
wss://stream.binance.com:9443/stream?streams=btcusdt@kline_1m/ethusdt@kline_1m/...
```
`QTSS_KLINE_SYMBOLS` (virgülle ayrılmış liste) env ile kontrol edilmeli.

### 3.4 EKSİKLİK: Copy trade yürütme eksik

**Mevcut:** `qtss-domain/copy_trade.rs` ve `qtss-storage/copy_trade.rs` domain tipleri var (`CopySubscription`, `CopyRule`). Ancak lider işlemini algılayıp takipçiye çoğaltacak yürütme katmanı yok.

**Gerekli:**
1. Lider pozisyon değişikliklerini izleyen `copy_trade_follower_loop`
2. `CopyRule` parametrelerine göre `OrderIntent` üretimi
3. `ExecutionGateway::place()` çağrısı

### 3.5 EKSİKLİK: Otomatik SL/TP yönetimi

**Mevcut:** `live_position_notify.rs` var ama yalnızca bildirim gönderiyor. Otomatik stop-loss ve take-profit emirleri vermiyor.

**Sorun:** `OrderType::StopMarket` ve `OrderType::TakeProfitMarket` domain'de tanımlı, `BinanceLiveGateway`'de implement edilmiş — ancak açık pozisyonları izleyip bu emirleri otomatik veren bir loop yok.

**Gerekli:** `position_manager_loop` — açık pozisyonları DB'den okur, SL/TP tetiklendiyse `ExecutionGateway::cancel()` + yeni emir verir.

### 3.6 EKSİKLİK: Kill switch / drawdown koruması

**Mevcut:** Yok.

**Gerekli:** `QTSS_MAX_DRAWDOWN_PCT` env değişkeni ile günlük/haftalık drawdown limiti. Limit aşılınca tüm açık pozisyonlar market emirle kapatılır ve yeni emir açılması engellenir.

### 3.7 EKSİKLİK: Funding rate arbitraj stratejisi

**Mevcut:** `score_binance_premium_funding()` ve `score_hl_meta_asset_ctxs_for_coin()` hazır skor fonksiyonları var. Ancak spot–futures fark ticareti yapan strateji yok.

**Gerekli:** Binance funding rate > eşik olduğunda spot AL + futures SHORT (veya tersi) yapan strateji.

### 3.8 EKSİKLİK: Nansen `perp-leaderboard` → watchlist pipeline

**Mevcut:** `perp-leaderboard` endpoint hiç çağrılmıyor.

**Sorun:** `profiler/perp-positions` çağrısı anlamlı olabilmesi için izlenecek cüzdanlar listesi (watchlist) gerekiyor. Bu liste `perp-leaderboard`'dan otomatik oluşturulabilir.

**Gerekli:**
1. `perp_leaderboard_loop` — haftada 1 çalışır, top 20 cüzdanı `app_config`'e yazar
2. `nansen_whale_watchlist_loop` — watchlist'teki cüzdanlar için `profiler/perp-positions` çağırır
3. Sonuç `hl_whale_score` kolonuna yazılır (şu an her zaman `None`)

### 3.9 KOD KALİTESİ: `score_nansen_smart_money_depth` ismi yanıltıcı

Fonksiyon aslında Nansen'in döndürdüğü **satır sayısını** skora çeviriyor. Bu "smart money depth" değil, sadece API yanıtının doluluk oranı. Rename: `score_nansen_response_coverage` veya tamamen kaldır.

### 3.10 KOD KALİTESİ: `confluence.rs` boş iskelet

`qtss-worker/src/confluence.rs` var ama içi neredeyse boş. `market_confluence_snapshots` tablosu mevcut (migration 0027, 0029) ama buraya hiçbir şey yazılmıyor. Confluence skoru `onchain_signal_scorer.rs` içinde hesaplanıyor ancak `market_confluence_snapshots`'a yazılmıyor — bu tutarsızlık giderilmeli.

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
- `/api/v1/smart-money/netflows`
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

### ADIM 3: nansen_engine.rs genişlemesi

**Dosya:** `crates/qtss-worker/src/nansen_engine.rs`

Mevcut `nansen_token_screener_loop`'u şablon olarak kullan. Her yeni endpoint için ayrı loop fonksiyonu:

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

**main.rs'e eklenecek spawn'lar:**

```rust
let nansen_nf_pool = pool.clone();
tokio::spawn(nansen_engine::nansen_netflows_loop(nansen_nf_pool));

let nansen_pt_pool = pool.clone();
tokio::spawn(nansen_engine::nansen_perp_trades_loop(nansen_pt_pool));

let nansen_fi_pool = pool.clone();
tokio::spawn(nansen_engine::nansen_flow_intel_loop(nansen_fi_pool));
```

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

**Dosya:** `migrations/0032_nansen_extended_scores.sql`

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

---

### ADIM 7: qtss-strategy crate (yeni crate — en kritik eksiklik)

**Klasör:** `crates/qtss-strategy/`

**Cargo.toml eklentisi (workspace):**
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

**crates/qtss-strategy/src/context.rs:**
```rust
use qtss_storage::OnchainSignalScoreRow;
use rust_decimal::Decimal;

/// Her strateji döngüsünün okuduğu piyasa bağlamı.
/// DB'den tek seferde derlenir, stratejilere Arc ile paylaşılır.
#[derive(Debug, Clone)]
pub struct MarketContext {
    pub symbol: String,
    pub aggregate_score: f64,       // onchain_signal_scores.aggregate_score
    pub confidence: f64,            // onchain_signal_scores.confidence
    pub direction: String,          // "strong_buy" | "buy" | "neutral" | "sell" | "strong_sell"
    pub conflict_detected: bool,    // TA vs on-chain çelişkisi
    pub market_regime: Option<String>, // "RANGE" | "TREND" | "KOPUS" | "BELIRSIZ"
    pub funding_score: Option<f64>,
    pub nansen_perp_score: Option<f64>,
    pub last_close: Option<Decimal>, // market_bars'dan son kapanış fiyatı
}

impl MarketContext {
    /// onchain_signal_scores'dan yükle.
    pub async fn load(pool: &sqlx::PgPool, symbol: &str) -> Option<Self> {
        // fetch_latest_onchain_signal_score kullan
        todo!()
    }
}
```

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

**Dosya:** `crates/qtss-worker/src/main.rs`

```rust
// Mevcut tekil QTSS_KLINE_SYMBOL yerine:
// QTSS_KLINE_SYMBOLS=BTCUSDT,ETHUSDT,SOLUSDT,BNBUSDT

if let Ok(symbols_raw) = std::env::var("QTSS_KLINE_SYMBOLS") {
    let symbols: Vec<String> = symbols_raw.split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    if !symbols.is_empty() {
        let interval = std::env::var("QTSS_KLINE_INTERVAL").unwrap_or_else(|_| "1m".into());
        let segment = std::env::var("QTSS_KLINE_SEGMENT").unwrap_or_else(|_| "futures".into());
        
        // Combined stream URL:
        // wss://stream.binance.com:9443/stream?streams=btcusdt@kline_1m/ethusdt@kline_1m
        tokio::spawn(multi_kline_ws_loop(symbols, interval, segment, pool.clone()));
    }
}
```

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

# Position manager
QTSS_POSITION_MANAGER_TICK_SECS=10
QTSS_DEFAULT_STOP_LOSS_PCT=2.0          # yüzde olarak
QTSS_DEFAULT_TAKE_PROFIT_PCT=4.0
```

---

## 6. Migration numaralandırma kuralı

**ÖNEMLİ:** Migration dosyalarını eklemeden önce son numarayı kontrol et:
```bash
ls migrations/*.sql | sort | tail -5
```

Şu an son migration: `0031_onchain_signal_weights_app_config.sql`

Yeni migration eklenince: `0032_...`, `0033_...` şeklinde sıralı ilerle. Aynı prefix'li iki dosya açmak SQLx'i bozar.

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

| Adım | Dosya/Crate | Öncelik | Süre tahmini |
|------|-------------|---------|--------------|
| 1 | qtss-nansen: 5 yeni endpoint fonksiyonu | Yüksek | 2 saat |
| 2 | registry.rs: yeni source_key'ler | Düşük | 15 dk |
| 3 | nansen_engine.rs: 3 yeni loop | Yüksek | 3 saat |
| 4 | signal_scorer.rs: 4 yeni score_fn | Yüksek | 2 saat |
| 5 | onchain_signal_scorer.rs: yeni bileşenler | Orta | 1 saat |
| 6 | Migration 0032 | Orta | 30 dk |
| 7 | qtss-strategy crate (iskelet + signal_filter) | Kritik | 1 gün |
| 8 | Çoklu sembol WS | Orta | 3 saat |
| 9 | position_manager.rs | Yüksek | 4 saat |
| 10 | kill_switch.rs | Yüksek | 2 saat |

**Önce 1-6 arası adımları tamamla** — bunlar mevcut altyapıyı Nansen endpoint'lerle zenginleştirir, risk almadan test edilebilir.

**Sonra 7'yi yaz** — strateji crate'i tüm diğer parçaları bir araya getirir. Bu adım en uzun sürer ve en dikkatli tasarım gerektirir.

**8-10 paralel geliştirilebilir** — birbirinden bağımsız.

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
Mevcut tasarım: sinyal üretiliyor ama `conflict_detected` bilgisi API'ye iletiliyor. Strateji katmanında: `conflict_detected=true` ise pozisyon büyüklüğünü `0.5×` ile çarp (half-size) veya pozisyon açma. Bu karar strateji crate'inde implement edilmeli.
