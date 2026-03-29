# QTSS — Cursor Geliştirme Rehberi

> **Bu doküman ne için?**
> Cursor'ın QTSS codebase'ini anlayıp doğru genişletmeler yapabilmesi için hazırlanmıştır.
> Mevcut kodun anatomisi, **§3**’te arşivlenmiş (çoğu giderilmiş) tespit notları, **§4** sıralı ADIM planı (bu depoda **uygulanmış**), migrasyon ve env kuralları içerir.
> Önce **§0** (kesin kural + tamamlanma özeti), gerekiyorsa **§2.1** / **§4** / **§6**; ayrıntı için ilgili alt başlıklar.

---

## 0. Proje özeti

**Kesin kural (doküman ↔ kod hizalama):** Aynı değişiklikte şunlar tutarlı kalmalı — `migrations/README.md` SQL envanteri; kök **`.env.example`**; **`crates/qtss-worker/src/main.rs`** (`tokio::spawn` sırası, `init_logging`, `run_migrations` `.context`); **`crates/qtss-api/src/main.rs`** (`init_logging`, `run_migrations` `.context` — worker ile aynı sorun giderme işaretleri); **`crates/qtss-worker/src/data_sources/registry.rs`**; **`crates/qtss-storage`** (`run_migrations`, checksum / çift önek / `bar_intervals` — hata metni `pool.rs`); **`docs/PROJECT.md`** (§7 migrasyon, Nansen notları); **`docs/SECURITY.md`** (worker canlı otomatik emir / `exchange_accounts`); bu belgenin **§2.1** (spawn, varsayılan log, registry), **§4** (ADIM 1–10 / özellik–dosya eşlemesi), **§5** (env tek kaynak = `.env.example`), **§6** (migrasyon) bölümleri. Web istemcisi: **`web/.env.example`** yalnız Vite/API; worker ortamı kök şablonda.

**Tamamlanma (bu depo):** **§4 ADIM 1–10** ile **§3** tespit maddeleri (§3.5 SL/TP + dry/live `position_manager` dahil) **kodda karşılanmıştır** — ayrıntı §4/§9 tablosu. Belge dışı ürün işleri (**strateji canlı gateway onayı**, yeni migration, operasyon limitleri) **§9 «Sonraki işler»** ve **`docs/PROJECT.md`** yol haritasında kalır; bunlar bu rehberin «sıralı ADIM» setinin parçası değildir.

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
├── qtss-nansen          # Nansen HTTP (screener + smart-money / TGM / perp-pnl-leaderboard + profiler perp-positions)
├── qtss-notify          # Bildirim kanalları (Telegram, webhook, …)
├── qtss-strategy        # Strateji (signal_filter, whale_momentum, arb_funding, copy_trade, risk)
└── qtss-chart-patterns  # Elliott Wave, ACP pattern taraması
```

**Migrasyonlar:** `migrations/*.sql` — geliştirme makinesi ile üretim/CI aynı dosya setini kullanmalı; `ls migrations/*.sql | wc -l` / `git ls-files 'migrations/*.sql' | wc -l` çıktısı **`migrations/README.md` envanter satır sayısı ile aynı** olmalı (uyumsuzsa repo senkronu eksik). **Sıralı envanter:** `migrations/README.md`. Özet şema notları: `docs/PROJECT.md` §7; sürüm numarası ve çift önek yasağı: **§6** (bu belge). **IDE / WSL:** Bazı editör workspace görünümleri `migrations` altında yalnızca dosyaların bir alt kümesini indeksleyebilir; sayım ve envanter doğrulaması shell’deki `ls` / `git ls-files` çıktısına güvenin (`migrations/README.md` ile eşleşmeli).

---

## 1. Mevcut veri akışı — tam harita

```
Dış API'ler
    │
    ├─ Nansen API ──────────► nansen_engine.rs (token screener → `nansen_snapshots` + `data_snapshots`)
    │                         nansen_extended.rs (netflows, holdings, perp-trades, TGM, leaderboard, … → `data_snapshots` + watchlist)
    │                              ▼
    │                         nansen_snapshots (DB); data_snapshots (DB, birden çok `source_key`; registry §2.1)
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
// Mevcut spawn sırası (`main.rs`, DATABASE_URL varken). Gerçek kodda her görev `pool.clone()` ile ayrı değişkene alınır (`pnl_pool`, `engine_pool`, …); sıra aşağıdaki ile aynıdır.
tokio::spawn(pnl_rollup_loop(pool.clone()));
tokio::spawn(binance_spot_reconcile::binance_spot_reconcile_loop(pool.clone())); // QTSS_RECONCILE_BINANCE_SPOT_ENABLED
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

Varsayılan log filtresi: `init_logging("info,qtss_worker=debug,sqlx::postgres::notice=warn")` — `_sqlx_migrations` Postgres NOTICE satırlarını INFO’da göstermez.

**Registry (Phase G):** `REGISTERED_DATA_SOURCES` uzunluğu **9** (8 sabit `source_key` + `*` harici kaynak jokeri); `REGISTERED_NANSEN_HTTP_KEYS` **8** giriş (token screener + extended snapshot anahtarları, `nansen_perp_leaderboard` dahil). Çalışan log: `count=` ile doğrulanır.

**`qtss-api/src/main.rs`:** `run_migrations` sırasında aynı NOTICE tekrarını kesmek için `init_logging` içinde **`sqlx::postgres::notice=warn`** (worker ile aynı hedef); tam dize: `info,qtss_api=debug,qtss_storage=debug,tower_http=info,sqlx::postgres::notice=warn`.

`run_migrations` hata ayrıntısı: `crates/qtss-storage/src/pool.rs` (checksum, çift önek, `bar_intervals` eksikliği); üzerine worker ve **qtss-api** `main.rs` içindeki `anyhow::Context` bağlam satırları (checksum / `0036` / çift önek → **§6**).

### 2.2 qtss-nansen/src/lib.rs

Nansen HTTP istemcisi. `post_json_path` + `apiKey` header. Mevcut çağrılar: `post_token_screener`, `post_smart_money_netflows` (**yol:** `api/v1/smart-money/netflow` — çoğul `/netflows` birçok planda **404**), `post_smart_money_holdings`, `post_smart_money_perp_trades`, `post_tgm_flow_intelligence`, `post_tgm_who_bought_sold`, `post_profiler_perp_leaderboard` (göreli yol; worker varsayılanı **`api/v1/tgm/perp-pnl-leaderboard`** — `NANSEN_PERP_LEADERBOARD_PATH`; eski `profiler/perp-leaderboard` 404), `post_profiler_perp_positions`.

İstek gövdeleri: `smart-money/netflow` ve `holdings` için **`chains`** dizisi; `tgm/who-bought-sold` için **`chain`**; `tgm/perp-pnl-leaderboard` için **`token_symbol`** + **`date`** (veya `NANSEN_PERP_LEADERBOARD_BODY_JSON`) — bkz. `nansen_extended.rs` ve kök `.env.example`.

### 2.3 qtss-worker/src/signal_scorer.rs

`score_for_source_key(source_key, response) -> f64` dispatch fonksiyonu. Yeni Nansen endpoint eklendiğinde buraya da `if source_key == "nansen_netflows" { ... }` (veya ilgili `source_key`) eklenmeli.

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

### 3.5 ~~EKSİKLİK: Otomatik SL/TP yönetimi~~ (giderildi)

**Durum:** `position_manager_loop` (`QTSS_POSITION_MANAGER_ENABLED`): `exchange_orders` dolumlarından **net long** tahmini, `market_bars` son kapanışa göre SL/TP eşiği. **Dry:** `QTSS_POSITION_MANAGER_DRY_CLOSE_ENABLED=1` → `DryRunGateway` ile simüle market kapatma. **Canlı:** `QTSS_POSITION_MANAGER_LIVE_CLOSE_ENABLED=1` (dry kapalıyken) → `exchange_accounts` + `BinanceLiveGateway` ile **reduce-only** futures market satışı (spot’ta satış); emir `exchange_orders`’a yazılır. `qtss_common::is_trading_halted()` iken canlı kapatma **atlanır**. Mum aralığı: `QTSS_POSITION_MANAGER_BAR_INTERVAL` (varsayılan `1m`). Canlı gerçekleşen emir özeti: `live_position_notify.rs`.

### 3.6 ~~EKSİKLİK: Kill switch / drawdown koruması~~ (giderildi)

**Durum:** `kill_switch_loop` + `qtss_common::halt_trading` / `is_trading_halted`; `QTSS_KILL_SWITCH_ENABLED`, `QTSS_MAX_DRAWDOWN_PCT` (referans equity ile günlük realized eşiği). **Not:** `kill_switch` döngüsü açık pozisyonları **kapatmaz**; amaç yeni emirleri `is_trading_halted` ile durdurmak. İsteğe bağlı otomatik market kapatma ayrıca §3.5 (`QTSS_POSITION_MANAGER_LIVE_CLOSE_ENABLED`); canlı yol `is_trading_halted` iken çalışmaz. Manuel: API `POST .../orders/binance/place` veya borsa arayüzü.

### 3.7 ~~EKSİKLİK: Funding rate arbitraj stratejisi~~ (giderildi)

**Durum:** `qtss-strategy::arb_funding` + dry gateway; eşik env (`QTSS_ARB_FUNDING_THRESHOLD` vb.). Bkz. ADIM 7 / `.env.example`.

### 3.8 ~~EKSİKLİK: Nansen perp leaderboard → watchlist pipeline~~ (giderildi)

**Durum:** `nansen_perp_leaderboard_loop` → `app_config` (`NANSEN_WHALE_WATCHLIST_KEY`); `nansen_whale_perp_aggregate_loop` → `profiler/perp-positions` birleşik yanıt → `nansen_whale_perp_aggregate` snapshot → skorlama `hl_whale_score`. Leaderboard: `NANSEN_PERP_LEADERBOARD_PATH` (varsayılan `api/v1/tgm/perp-pnl-leaderboard`) + gövde (`NANSEN_PERP_LEADERBOARD_BODY_JSON` veya `TOKEN_SYMBOL` / `LOOKBACK_DAYS`).

### 3.9 ~~KOD KALİTESİ: `score_nansen_smart_money_depth`~~ (giderildi)

**Durum:** `signal_scorer.rs` içinde `score_nansen_response_coverage`; eski yanıltıcı isim kaldırıldı.

### 3.10 ~~KOD KALİTESİ: `confluence.rs` boş iskelet~~ (giderildi)

`qtss-worker/src/confluence.rs` rejim ağırlıkları + `data_snapshots` ile bileşik skor üretir; `insert_market_confluence_snapshot` + `upsert_analysis_snapshot` (`engine_kind = confluence`) ile kalıcı yazım yapılır.

---

## 4. Geliştirme planı — sıralı adımlar

Aşağıdaki ADIM 1–10, projeyi genişletirken kullanılan **sıralı plandır**; bu repoda ilgili parçalar **uygulanmış** durumdadır. **§3** tespit listesindeki maddeler de (§3.5 dahil) bu kapsamda **giderilmiş** sayılır; ürün genelinde süren işler **§9 «Sonraki işler»** ile `docs/PROJECT.md` yol haritasında kalır. Yeni özellik eklerken §6 migrasyon kuralı ve `migrations/README.md` envanterine uy.

---

### ADIM 1: qtss-nansen crate genişlemesi — **uygulandı**

**Dosya:** `crates/qtss-nansen/src/lib.rs` — `post_json_path` + `apiKey` header; `NansenError` (`is_insufficient_credits`, `http_status`).

| Fonksiyon | Sabit yol / not |
|-----------|------------------|
| `post_token_screener` | `api/v1/token-screener` |
| `post_smart_money_netflows` | `api/v1/smart-money/netflow` (çoğul `/netflows` sık 404) |
| `post_smart_money_holdings` | `api/v1/smart-money/holdings` |
| `post_smart_money_perp_trades` | `api/v1/smart-money/perp-trades` |
| `post_tgm_flow_intelligence` | `api/v1/tgm/flow-intelligence` |
| `post_tgm_who_bought_sold` | `api/v1/tgm/who-bought-sold` |
| `post_profiler_perp_leaderboard` | göreli yol parametresi (worker varsayılanı `api/v1/tgm/perp-pnl-leaderboard`) |
| `post_profiler_perp_positions` | `api/v1/profiler/perp-positions` |

---

### ADIM 2: data_sources registry — **uygulandı**

**Dosya:** `crates/qtss-worker/src/data_sources/registry.rs` — `NANSEN_*_DATA_KEY` sabitleri, `REGISTERED_DATA_SOURCES` (9 satır: 8 sabit + `*`), `REGISTERED_NANSEN_HTTP_KEYS` (8 anahtar). Tek doğruluk burada; §2.1 ile aynı.

---

### ADIM 3: Nansen genişletilmiş HTTP döngüleri — **uygulandı**

**Dosyalar:** `nansen_extended.rs` (gövde), `nansen_engine.rs` (token screener + **re-export**), `main.rs` spawn sırası §2.1.

Uygulanmış loop’lar: `nansen_netflows_loop`, `nansen_holdings_loop`, `nansen_perp_trades_loop`, `nansen_who_bought_loop`, `nansen_flow_intel_loop`, `nansen_perp_leaderboard_loop`, `nansen_whale_perp_aggregate_loop`. Env ile kapatma: `NANSEN_*_ENABLED=0|false|off|no`. İstek gövdeleri ve TGM leaderboard: kök `.env.example` + §2.2.

---

### ADIM 4: signal_scorer.rs — **uygulandı**

**Dosya:** `crates/qtss-worker/src/signal_scorer.rs` — `score_nansen_netflows`, `score_nansen_perp_direction`, `score_nansen_flow_intelligence`, `score_nansen_buyer_quality`, `score_nansen_dex_buy_sell_pressure`, `score_for_source_key` içinde `nansen_netflows` / `nansen_perp_trades` / `nansen_flow_intelligence` / `nansen_who_bought_sold` / `nansen_token_screener` eşlemesi. Bileşen ağırlıkları `app_config` `onchain_signal_weights` JSON’undan (`nansen_netflows`, `nansen_perp`, `nansen_buyer_quality`, …) — seed migrasyon **0031**; kolonlar **0030** / **0032** vb. §6 tablosuna bak.

---

### ADIM 5: onchain_signal_scorer.rs — **uygulandı**

**Dosya:** `crates/qtss-worker/src/onchain_signal_scorer.rs` — `compute_and_persist_for_symbol` içinde Nansen netflow / perp / flow-intel / buyer quality bileşenleri, `hl_whale_score`, TA `signal_dashboard` çatışma işareti. Nansen flow-intelligence ile Coinglass netflow **çakışması**: §3.2 (etkin ağırlık yarıya indirme + `meta_json`). Kalıcı şema: `qtss-storage` `onchain_signal_scores` + migrasyonlar.

---

### ADIM 6: SQL migrasyonları — **uygulandı**

**Tam zincir:** `migrations/README.md` envanteri (0001–0036). Özet: `0029`–`0033` confluence + on-chain + Nansen genişletme skorları + HL whale ağırlığı; **`0034`/`0035`** `engine_symbols` FK; **`0036`** `bar_intervals` eksik telafisi. Çift `0013`/`0014` öneki yasağı ve checksum: **§6**.

**Tarihsel referans** (fikir özeti; gerçek DDL ilgili `00xx_*.sql` dosyalarında):

```sql
-- Örnek: Nansen genişletilmiş skor kolonları (0032 vb. dosyada uygulanmış olabilir)
ALTER TABLE onchain_signal_scores
    ADD COLUMN IF NOT EXISTS nansen_netflow_score DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS nansen_perp_score    DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS nansen_buyer_quality_score DOUBLE PRECISION;
-- app_config `onchain_signal_weights` + `nansen_whale_watchlist` — migrasyon/seed ile
```

Yeni şema değişikliği: **yeni numara** (`0037_*.sql`); uygulanmış dosyayı düzenleme.

---

### ADIM 7: qtss-strategy crate — **uygulandı**

**Klasör:** `crates/qtss-strategy/` — modüller: `signal_filter`, `whale_momentum`, `arb_funding`, `copy_trade`, `risk`, `context`, `conflict_policy`. Workspace üyesi ve `qtss-strategy` path bağımlılığı kök `Cargo.toml` içinde.

**Giriş noktaları** (hepsi `pub async fn run(pool: PgPool, gateway: Arc<dyn ExecutionGateway>)`):

| Modül | Özet | Önemli env |
|--------|------|------------|
| `signal_filter` | `MarketContext` + eşikler; `QTSS_SIGNAL_FILTER_AUTO_PLACE`, `ON_CONFLICT`, `BRACKET_ORDERS`; `is_trading_halted` | `QTSS_LONG_THRESHOLD`, `QTSS_SHORT_THRESHOLD`, `QTSS_SIGNAL_FILTER_TICK_SECS`, `QTSS_STRATEGY_ORDER_QTY`, Kelly: `QTSS_KELLY_*` |
| `whale_momentum` | Nansen netflow + perp skorları, funding “crowding” filtresi | `QTSS_WHALE_MOMENTUM_TICK_SECS`, `QTSS_WHALE_MOMENTUM_THRESHOLD`, `QTSS_WHALE_FUNDING_CROWDING_BLOCK`, `QTSS_WHALE_MOMENTUM_AUTO_PLACE` |
| `arb_funding` | `data_snapshots` Binance premium funding; isteğe bağlı dry iki bacak | `QTSS_ARB_FUNDING_THRESHOLD`, `QTSS_ARB_FUNDING_TICK_SECS`, `QTSS_ARB_FUNDING_DRY_TWO_LEG`, `QTSS_ARB_FUNDING_ORDER_QTY` |
| `copy_trade` | `nansen_perp_trades` snapshot yönü + `CopyRule` | `QTSS_COPY_TRADE_STRATEGY_TICK_SECS`, `QTSS_COPY_TRADE_DIRECTION_THRESHOLD`, `QTSS_COPY_TRADE_BASE_QTY` |

**`risk.rs`:** Kelly ölçekli miktar (`apply_kelly_scale_to_qty`), `DrawdownGuard`, `clamp_qty_by_max_notional_usdt` — env ile uyumlu.

**Worker entegrasyonu:** `crates/qtss-worker/src/strategy_runner.rs` — `QTSS_STRATEGY_RUNNER_ENABLED=1` iken `DryRunGateway` + dört `tokio::spawn` (`signal_filter::run`, `whale_momentum::run`, `arb_funding::run`, `copy_trade::run`). Varsayılan: kapalı.

**Kill switch:** Strateji döngüleri `qtss_common::is_trading_halted()` ile uyumlu; global durum `qtss_common::{halt_trading, is_trading_halted}` (ADIM 10).

---

### ADIM 8: Çoklu sembol WebSocket — **uygulandı**

**Dosya:** `crates/qtss-worker/src/main.rs` — `QTSS_KLINE_SYMBOLS` tanımlıysa `multi_kline_ws_loop` + `public_*_combined_kline_url`; aksi halde `QTSS_KLINE_SYMBOL` ile tekil akış. Semboller **büyük harf** (örn. `BTCUSDT`) kabul edilir; URL üretimi crate içinde küçük harfe çevrilir.

---

### ADIM 9: Position manager loop — **uygulandı**

**Dosya:** `crates/qtss-worker/src/position_manager.rs`

- **`pub async fn position_manager_loop(pool: PgPool)`** — imzada gateway yok; içeride `DryRunGateway` ve/veya `BinanceLiveGateway` env ile seçilir.
- **Dry kapatma:** `QTSS_POSITION_MANAGER_DRY_CLOSE_ENABLED=1` → simüle `place`. **Canlı kapatma:** `QTSS_POSITION_MANAGER_LIVE_CLOSE_ENABLED=1` ve dry **kapalı** → kullanıcı `exchange_accounts` (Binance, ilgili segment) ile gerçek reduce-only market emri + `insert_submitted`. **`is_trading_halted()`** iken canlı yol çalışmaz.
- `exchange_orders` dolumlarından net long; `market_bars` son kapanış; SL/TP: `QTSS_DEFAULT_STOP_LOSS_PCT`, `QTSS_DEFAULT_TAKE_PROFIT_PCT`; bar aralığı: **`QTSS_POSITION_MANAGER_BAR_INTERVAL`**.
- Aç/kapa: **`QTSS_POSITION_MANAGER_ENABLED`**; tick: **`QTSS_POSITION_MANAGER_TICK_SECS`**.

---

### ADIM 10: Kill switch — **uygulandı**

**Dosya:** `crates/qtss-worker/src/kill_switch.rs`

- **`pub async fn kill_switch_loop(pool: PgPool)`** — gateway parametresi yok; **açık pozisyonları otomatik kapatmaz** (§3.6).
- Günlük realized P&L toplamı `sum_today_daily_realized_pnl`; eşik altına inince **`qtss_common::halt_trading`**. Global bayrak: **`qtss_common::is_trading_halted`** (worker’da `kill_switch::is_halted` yok — strateji ve diğer kod `qtss_common` kullanır).
- **Env:** `QTSS_KILL_SWITCH_ENABLED`, `QTSS_KILL_SWITCH_TICK_SECS`, `QTSS_MAX_DRAWDOWN_PCT` + `QTSS_KILL_SWITCH_REFERENCE_EQUITY_USDT` veya `QTSS_KILL_SWITCH_DAILY_LOSS_USDT`.

**Strateji / emir öncesi kontrol örneği:**

```rust
if qtss_common::is_trading_halted() {
    tracing::warn!("trading halted — emir verilmiyor");
    continue;
}
```

---

## 5. Ortam değişkenleri — tam liste

**Kaynak (kesin):** Tüm anahtarlar, yorumlar ve satır sırası kök **`.env.example`** içindedir; web istemcisi: **`web/.env.example`**. Bu bölümde tam env listesi **tekrarlanmaz** — çift kaynak drift’e yol açmaması için §0’daki kesin kurala uygun olarak yalnız özet notlar.

**Şema:** SQLx migrasyonları — numara kuralları **§6**; `public.bar_intervals` eksikliği için **`0036_bar_intervals_repair_if_missing.sql`** ve §10 SSS.

**Loglama:** `RUST_LOG` tanımlıysa `qtss_common::init_logging` varsayılan dizgeyi **geçersiz kılar** (`EnvFilter::try_from_default_env`). `RUST_LOG` yokken worker/API varsayılan dizgeleri **§2.1**. JSON satır log: `QTSS_LOG_JSON=1` (kök `.env.example` başındaki yorum satırlarıyla aynı örnek).

```bash
# Kök .env.example başı ile aynı (kopyalarken şablon dosyasını kullanın):
# qtss-worker varsayılanına yakın:
# RUST_LOG=info,qtss_worker=debug,sqlx::postgres::notice=warn
# qtss-api varsayılanına yakın:
# RUST_LOG=info,qtss_api=debug,qtss_storage=debug,tower_http=info,sqlx::postgres::notice=warn
# QTSS_LOG_JSON=1
```

Nansen genişletme, strateji dry, kill switch, position manager ve diğer tüm `QTSS_*` / `NANSEN_*` anahtarları: `.env.example` içindeki `# --- QTSS_CURSOR_DEV_GUIDE.md §5` bölümünden itibaren.

---

## 6. Migration numaralandırma kuralı

### 6.1 SQLx sürümü = dosya adındaki sayı

`sqlx::migrate!` dosya adının **ilk alt çizgiye kadar olan sayısal önekini** `_sqlx_migrations.version` olarak kullanır. **0001–0036** tam dosya listesi: `migrations/README.md` (kesin envanter; yeni migration eklenince README güncellenmeli).

- `0029_market_confluence_payload_column.sql` → sürüm **29**
- `0030_onchain_signal_scores.sql` → sürüm **30**
- `0034_engine_symbols_fk_columns.sql` → sürüm **34**
- `0035_engine_symbols_bar_interval_fk_if_ready.sql` → sürüm **35**
- `0036_bar_intervals_repair_if_missing.sql` → sürüm **36**

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
| 36 | `bar_intervals_repair_if_missing` — `bar_intervals` yoksa oluşturur + `market_bars`/`engine_symbols` `bar_interval_id` doldurma |

Kontrol:

```sql
SELECT version, description FROM _sqlx_migrations ORDER BY version DESC LIMIT 10;
SELECT to_regclass('public.bar_intervals');  -- NULL ise `0036_bar_intervals_repair_if_missing.sql` uygulanmalı (migrate)
```

```bash
ls migrations/*.sql | sort | tail -10
```

### 6.2 Aynı önek iki kez kullanılamaz (kritik)

Aynı numaralı iki dosya SQLx’i bozar; depoda **tek** önek kalmalı:

- **0013:** `0013_worker_analytics_schema.sql` — `bar_intervals` genişletmesi (eski `0013_bar_intervals.sql`) bu dosyada birleştirildi; ayrı `0013_*.sql` yok.
- **0014:** yalnızca `0014_catalog_fk_columns.sql` (eski `0014_engine_symbols_catalog_fk_repair.sql` kaldırıldı).

`0034_engine_symbols_fk_columns.sql` eksik FK kolonları için idempotent `ADD COLUMN` sağlar (`bar_intervals` yoksa `bar_interval_id` atlanır). `0035_engine_symbols_bar_interval_fk_if_ready.sql` tablo sonradan oluşunca tamamlar. `0036_bar_intervals_repair_if_missing.sql` — `bar_intervals` tablosu yokken 0013 kayıtlı gibi kalan DB’ler için telafi. Uygulanmış migrasyon dosyasını değiştirmeyin (§8 kural 6).

### 6.3 Yeni migration

Bir sonraki boş sürümü kullanın (ör. dizinde en yüksek önek 36 ise **`0037_yeni_ozellik.sql`**). Çift `0013_*.sql` / `0014_*.sql` açmayın.

---

## 7. Test stratejisi

### 7.1 Birim testler

`crates/qtss-worker/src/signal_scorer.rs` içinde `#[cfg(test)] mod tests` — örnekler: `nansen_netflows_bullish_dev_guide`, `nansen_perp_direction_heavy_long_dev_guide`, `dispatch_nansen_token_screener_uses_dex_pressure`, `nansen_dex_pressure`, `premium_funding` vb. Yeni skor fonksiyonu eklerken aynı modülde karşılık gelen `#[test]` ekleyin (`serde_json::json!` ile ham yanıt gövdesi).

```bash
cargo test -p qtss-worker
# Alt küme (örnek isim eşlemesi):
# cargo test -p qtss-worker nansen_netflows_bullish_dev_guide
```

### 7.2 Dry run testi

Strateji dry döngüleri: kök `.env` içinde **`QTSS_STRATEGY_RUNNER_ENABLED=1`** (ve `DATABASE_URL`). `strategy_runner` dört stratejiyi `DryRunGateway` ile başlatır; gerçek borsa çağrısı olmaz, paper ledger güncellenir.

```bash
QTSS_STRATEGY_RUNNER_ENABLED=1 cargo run -p qtss-worker
```

İş modu için ayrıca `AppMode::Dry` / `qtss-common` kullanımı bkz. `qtss-worker` ve `qtss-execution`.

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
| 6 | migrations `0013`→`0036` (worker, on-chain, confluence, `0034`–`0036` FK + `bar_intervals` telafisi) | Tamam; sunucu `ls migrations` ile doğrula; çift `0013`/`0014` önek yok |
| 7 | qtss-strategy + strategy_runner (dry) | Tamam |
| 8 | Çoklu sembol WS (`QTSS_KLINE_SYMBOLS`) | Tamam |
| 9 | position_manager.rs | Tamam (dry / isteğe bağlı live reduce-only; env ile kapalı) |
| 10 | kill_switch.rs | Tamam (env ile kapalı) |

**Sonraki işler:** yeni özellik = yeni migration numarası (§6); üretimde `DATABASE_URL` + Nansen/plan limitleri; strateji canlı gateway için ayrı onay ve risk kontrolleri.

### 9.1 Sonraki faz — `docs/PROJECT.md` §10

Bu rehberdeki ADIM 1–10 bittikten sonraki **ürün** işleri `docs/PROJECT.md` **§10 Yol haritası** üzerinden takip edilir. Özet sıra (oradaki numaralarla):

1. Kimlik / RBAC ince taneli izinler ve denetim derinliği — **kısmen:** JWT `permissions` (`qtss:read` \| `qtss:ops` \| `qtss:admin`), yeni tokenlarda claim; eski tokenlarda `require_jwt` sonrası `roles` ile doldurulur; `GET /api/v1/me` `permissions` döner; `require_admin` / `require_ops_roles` / `require_dashboard_roles` bu izinlere göre karar verir. Kullanıcı başına DB izinleri, daha ince stringler ve denetim derinliği sonraki iterasyon.  
2. Binance hesap bazlı gerçek ücret / trade fee API  
3. Emir + mutabakat (`reconcile`) sıklığı ve worker entegrasyonu — **kısmen:** periyodik **Binance spot** mutabakat döngüsü `binance_spot_reconcile.rs` (`QTSS_RECONCILE_BINANCE_SPOT_ENABLED`, `QTSS_RECONCILE_BINANCE_SPOT_TICK_SECS`); API `POST /api/v1/reconcile/binance` ile aynı `reconcile_binance_spot_open_orders`. Futures / otomatik durum güncelleme sonraki iterasyon.  
4. Copy trade — lider dolum dinleme + takipçi yürütme kuyruğu  
5. Analiz motoru ayrı crate  
6. AI onay kuyruğu  
7. Bildirim kuyruğu (`qtss-notify` ötesi)  
8. Web dashboard genişlemesi  
9. HA / observability  

**Önerilen bir sonraki teknik adım (küçük kapsam):** depoda **CI** ile `cargo check --workspace` (ve zaman içinde `cargo audit`) — `docs/SECURITY.md` §Bağımlılık. Büyük özellik seçimi: §10 satır 4 (copy trade kuyruk) veya satır 3 (reconcile) ürün önceliğine göre.

**§4 ile bu tablo:** Aynı ADIM numaraları; §4’te her adımın **uygulandı** özeti ve dosya/env referansları var.

---

## 10. Sık sorulan sorular

**S: Yeni Nansen endpoint ekleyince kaç kredi harcar?**
Her `smart-money/*` endpoint'i 5 kredi/çağrı. `tgm/*` endpoint'leri 1 kredi. 1800s tick ile günde ~48 çağrı × 5 kredi = ~240 kredi/gün/endpoint. 4 yeni endpoint = ~960 kredi/gün ek maliyet.

**S: Migration uygulandı mı kontrol nasıl yapılır?**
```sql
SELECT version, description FROM _sqlx_migrations ORDER BY version DESC LIMIT 5;
```

**S: `SELECT to_regclass('public.bar_intervals');` NULL dönüyor — ne yapmalıyım?**
0013 kayıtlı olsa bile tablo oluşmamış olabilir (eski migrasyon, el ile silme). Repoda **`0036_bar_intervals_repair_if_missing.sql`** varsa `cargo run -p qtss-api` veya worker ile bekleyen migrasyonları uygulayın; ardından `to_regclass` dolu olmalı. Bkz. §6.1 kontrol sorgusu ve `pool.rs` hata metni.

**S: Worker'ı yeniden başlatmadan config değiştirilebilir mi?**
`app_config` tablosundaki değerler (ağırlıklar, screener body) her döngüde DB'den okunur — restart gerekmez. Env değişkeni değişikliği ise restart gerektirir.

**S: Dry run modunda hangi işlemler gerçekleşir?**
`DryRunGateway` aktif olduğunda hiçbir gerçek emir Binance'a gitmez. Sadece `paper_fills` ve `paper_ledger` tabloları güncellenir. Veri toplama (Nansen, Coinglass, WS) dry run'da da çalışır.

**S: `conflict_detected=true` olduğunda strateji ne yapmalı?**
API tarafında bilgi dönüyor. `qtss-strategy::signal_filter` içinde `QTSS_SIGNAL_FILTER_ON_CONFLICT=half` (veya `none` / `skip`) ile çatışmada yarım miktar veya atlama uygulanır; bkz. `conflict_policy.rs`.

**S: `QTSS_POSITION_MANAGER_LIVE_CLOSE_ENABLED` güvenli mi?**
Yalnızca açıkça `1` yapıldığında ve `exchange_accounts` kaydı varken gerçek Binance emri üretir (reduce-only futures / spot satış). `is_trading_halted()` iken atlanır. Üretimde anahtar rotasyonu ve etki alanı: `docs/SECURITY.md`; dry ile önce doğrulayın (`QTSS_POSITION_MANAGER_DRY_CLOSE_ENABLED=1`).
