# QTSS — Master Geliştirme Rehberi (Cursor için)

> **Tarih:** 2026-03-29  
> **Amaç:** Projenin tam durum analizi, tespit edilen hatalar/sorunlar, iyileştirme önerileri ve **bulut + on-prem (kurum içi)** çoklu AI sağlayıcı destekli AI katmanı entegrasyon planını **tek çatı** altında birleştirir. Bu doküman Cursor'ın ana referansıdır.  
> **Önceki dokümanlar:** Eski ayrı rehberler (`SPEC_ONCHAIN_SIGNALS.md`, `PLAN_CONFLUENCE_AND_MARKET_DATA.md`, `DATA_SOURCES_AND_SOURCE_KEYS.md`, `NANSEN_TOKEN_SCREENER.md`) ve proje dışı `QTSS_AI_ENGINE_GUIDE` içeriği bu dosyada birleştirildi; o dosyalar repo’dan kaldırıldı. **`QTSS_CURSOR_DEV_GUIDE.md`** yeniden ince bir eşlik dosyası olarak tutulur (kaynak kod ve hata mesajlarındaki **§6** SQLx / checksum derin linkleri için). Güncel `docs/` envanteri için **Bölüm 11**’e bakın.

---

## İçindekiler

**0.** Durum özeti (**0.1**–**0.4**: kanıtlar, AI durumu, **minimal vs tam checkout**, **Elliott projeksiyon**) · **1.** Hatalar ve sorunlar · **2.** İyileştirme önerileri · **3.** AI planı (çoklu sağlayıcı) · **4.** FAZ 0–11 görev listesi · **5.** Migration kuralları · **6.** Ortam değişkenleri (AI + bildirim + DB config hedefi) · **7.** Test stratejisi (**7.1**–**7.7**: katmanlar, CI, yerel komutlar, checklist) · **8.** Kod kalitesi kuralları (**8.1** log seviyeleri) · **9.** Güvenlik · **10.** Worker spawn sırası · **11.** `docs/` dosya envanteri

*Önerilen okuma sırası (ilk oturum):* **0 → 1 → 2 → 4** (FAZ tabloları) → ihtiyaca göre **5–11**. Mimari özet için ayrıca `docs/PROJECT.md`.

---

## 0. Durum Özeti — Yönlendirmelerim Yapıldı mı?

**Kısa cevap:** **Evet — 17/17 temel yönlendirme tamam** (`qtss-ai` crate, `0042`–`0047` şeması — `0045`: `users.preferred_locale` + worker `system_config` tohumları; `0046`: paper/live pozisyon bildirim tick; `0047`: kill switch DB senkron + PnL poll tick; çoklu sağlayıcı + on-prem, API + web paneli, `system_config`, kapanışta `ai_decision_outcomes` geri bildirimi). Tasarım tek üreticiye kilitli değildir; **Anthropic**, **OpenAI-uyumlu iç uç**, **Ollama** vb. aynı trait ile seçilir. `ai_approval_requests` genel onay kuyruğu olarak `ai_decisions` zincirinden ayrı kalır.

**FAZ 0–11** (Bölüm 4) izleme tablolarıdır; **FAZ 0–8**, **FAZ 9 çekirdek i18n**, **FAZ 10** ve **FAZ 11** çekirdeği uygulanmıştır. **FAZ 9**’da `App.tsx` operatör metinleri anahtarlanır: çekmece/AI paneli, **üst şerit**, **ACP + kanal taraması**, **motor çekmecesi** (`app.engineDrawer.*` — DB snapshot, sinyal paneli tablosu `app.signalDashboard.*`, Paper/F4–F5 `app.paperDrawer.*`, snapshot/Confluence özetleri), komisyon/backfill/dev-login (`app.chartToolbar.*`, `app.channelReject.*`, `app.channelScan.*`, `app.acp.*`, `app.drawerPanel.*`, …). **Elliott** uzun yardım paragrafları, **bağlam / Nansen** çekmece metinleri ve `matchesSetting` arama anahtar kelimeleri kademeli kalabilir; API **`error_key`** için web istemci kataloğu genişletmesi isteğe bağlıdır. **FAZ 11.7** — outbox, PnL rollup, notify locale, paper/live pozisyon bildirim tick, **kill switch**, **engine analiz**, **on-chain sinyal**, **position manager**, **Nansen token screener + genişletilmiş HTTP döngüleri** (`nansen_extended`) için `system_config` + `resolve_worker_tick_secs` kodda (**`docs/CONFIG_REGISTRY.md`**). **Harici HTTP motorları** (`crates/qtss-worker/src/engines/*`): ortak uyku süresi **`worker.external_fetch_poll_tick_secs`** / env **`QTSS_EXTERNAL_FETCH_POLL_SECS`** (`external_common.rs`). Idempotent **0048+** tohumu yalnızca **`0044_system_config`** tablosu uygulanmış tam migration zincirinde anlamlıdır; **minimal checkout** (`0001`–`0012` yalnız) için env + kod varsayılanları (örn. 30s, min 10s) geçerlidir. Tamamlanan alt görev **✅ DONE** ile işaretlenir.

**Çalıştırma (yerel):** HTTP API → `cargo run -p qtss-api`; arka plan worker → `cargo run -p qtss-worker`. İkisi ayrı süreç; **PostgreSQL** ve `DATABASE_URL` zorunlu. İlk kurulumda örnek org/admin için: `cargo run -p qtss-api --bin qtss-seed`. Bağlantı ve gizli anahtarlar için kök `.env` / `.env.example` tek kaynak kabul edilir. Worker üzerinde `/live`, `/ready`, `/metrics` için HTTP dinleyici: `.env.example` içindeki **`QTSS_WORKER_HTTP_BIND`** (ör. `127.0.0.1:9090`). Web arayüzü: `web/` içinde `npm install`, `npm run dev` (geliştirme) / `npm run build` (üretim paketi); API adresi için `web/.env` veya Vite proxy ayarı `.env.example` / proje dokümanı ile uyumlu olmalıdır.

### 0.1 Tamamlanan Maddeler

| # | Yönlendirme | Durum | Dosya / Kanıt |
|---|-------------|-------|---------------|
| 1 | `signal_scorer.rs` — Nansen bileşenleri ayrı skor fonksiyonlarına | ✅ DONE | `score_nansen_netflows`, `_perp_direction`, `_flow_intelligence`, `_buyer_quality`, `_dex_buy_sell_pressure` (514 satır) |
| 2 | `onchain_signal_scorer.rs` — Coinglass/flow-intel çakışma yarı ağırlık | ✅ DONE | `coinglass_netflow_effective` mantığı + `meta_json` izleme (790 satır) |
| 3 | `data_sources/registry.rs` — kayıt sistemi | ✅ DONE | `REGISTERED_DATA_SOURCES` (9) + `REGISTERED_NANSEN_HTTP_KEYS` (8) |
| 4 | `nansen_extended.rs` — tüm HTTP loop'lar | ✅ DONE | 7 loop: netflows, holdings, perp_trades, who_bought, flow_intel, perp_leaderboard, whale_perp_aggregate |
| 5 | `qtss-strategy` crate — 4 strateji + risk | ✅ DONE | `signal_filter`, `whale_momentum`, `arb_funding`, `copy_trade`, `risk`, `conflict_policy`, `context` — `crates/qtss-strategy/src/*.rs` toplamı **1098** satır (`wc -l … \| tail -1`; asıl mantık modül dosyalarında) |
| 6 | `strategy_runner.rs` — DryRunGateway spawn | ✅ DONE | `spawn_if_enabled` + env kontrolü (61 satır) |
| 7 | `position_manager.rs` — SL/TP + dry/live close | ✅ DONE | Dry ve live yol ayrımı, `is_trading_halted()` kontrolü (383 satır) |
| 8 | `kill_switch.rs` — drawdown koruması | ✅ DONE | `qtss-common/src/kill_switch.rs`: `halt_trading` + `QTSS_MAX_DRAWDOWN_PCT` (25 satır) |
| 9 | `confluence.rs` — rejim ağırlıklı bileşik skor | ✅ DONE | `default_weights_by_regime`, `lot_scale_hint`, `direction_from_composite_score` (558 satır) |
| 10 | Çoklu sembol WS | ✅ DONE | `multi_kline_ws_loop` + combined URL |
| 11 | Copy trade kuyruğu | ✅ DONE | Migration 0037 + `copy_trade_queue.rs` + `copy_trade_follower.rs` |
| 12 | AI onay kuyruğu (basit) | ✅ DONE | Migration 0038 + API routes (list/create/decide) |
| 13 | Notify outbox | ✅ DONE | Migration 0039 + worker loop + API |
| 14 | User permissions + audit | ✅ DONE | Migration 0040-0041 + RBAC + admin CRUD |
| 15 | CI pipeline | ✅ DONE | `.github/workflows/ci.yml`: `qtss-storage`/`qtss-notify`/`qtss-common` lib + **`qtss-worker --bin qtss-worker`** + `cargo check` (api/worker); **`postgres-migrations`**: `migrations_apply` + **`qtss-ai` `decision_exists_for_hash_it`** + **`maybe_auto_approve_it`**; `web/`: `npm ci`, `i18n:check`, `build` |
| 16 | Probe endpoints | ✅ DONE | Worker: `/live`, `/ready`, `/metrics` — `QTSS_WORKER_HTTP_BIND` ile açılır (`.env.example`) |

*Kanıt sütunundaki satır sayıları `wc -l <dosya>` ile ölçülür; üstteki rakamlar 2026-03 repo durumuyla uyumludur (`signal_scorer` 514, `onchain_signal_scorer` 790, `qtss-strategy` src toplamı 1098, `strategy_runner` 61, `position_manager` 383, `confluence` 558, `kill_switch` 25). `qtss-strategy` için: `wc -l crates/qtss-strategy/src/*.rs | tail -1`. Crate toplamları değiştikçe tablo güncellenmelidir.*

### 0.2 AI katmanı (önceki “tek eksik” — durum)

| # | Yönlendirme | Durum | Kanıt |
|---|-------------|-------|--------|
| 17 | **AI katmanı (`qtss-ai` + çoklu sağlayıcı, on-prem dahil)** | ✅ DONE | `crates/qtss-ai/` (`AiRuntime`, `providers/*`, `context_builder`, katman süpürüleri, `feedback`); AI tabloları `migrations/0001_qtss_baseline.sql` içinde; API `routes/ai_decisions.rs`, `web/…/AiDecisionsPanel.tsx`, worker `ai_engine.rs` + `position_manager` AI SL/TP + `record_decision_outcome` |

### 0.3 Migrasyon düzeni (bu depo — kod doğruluk kaynağı)

| Gerçek durum | Açıklama |
|--------------|----------|
| **Tek dosya baseline** | `migrations/0001_qtss_baseline.sql` — SQLx tek sürüm kaydı; içinde eski çok dosyalı zincir `-- >>> merged from: NNNN_….sql` başlıklarıyla birleşiktir (`0007`–`0012` ACP satırları, Nansen, `engine_symbols`, `system_config`, AI tabloları, `exchange_fills`, …). |
| **Yeni şema değişikliği** | Geçici `0002_*.sql` → `python3 scripts/squash_migrations_into_one.py` ile tekrar tek baseline (tam akış: `migrations/README.md`). |
| **Tarihsel not** | Eski dallarda ayrı `0001_init.sql` … `0012_*.sql` + sonrası `0013`+ dosyaları vardı; **mevcut ağaçta** bunlar ayrı dosya olarak yoktur — kanıt: `ls migrations/*.sql` ve baseline içi `merged from` yorumları. |

`docs/PROJECT.md` §7 ve bu bölüm aynı hikâyeyi anlatmalıdır; belge ile `migrations/*.sql` çelişmemelidir.

### 0.4 Elliott ileri projeksiyon (web, `elliottEngineV2`)

Grafik üzerindeki **ileri Fib / Elliott projeksiyon** çizgileri `web/src/lib/elliottEngineV2/projection.ts` içindeki `buildElliottProjectionOverlayV2` ile üretilir (`zigzagKind: elliott_projection`). Zaman–fiyat çapası **kaynak zaman diliminin** (`sourceTf`) `ohlcByTf` son mumuna hizalanır; segment süreleri fiyat adımına ve itkı/düzeltme hızına göre ölçeklenir; son ~40 mumun hızına göre rejim çarpanı süreyi kısaltır veya uzatır; işaret metinlerinde birikimli ETA (`+Nm`, `+Nh`, `+Nd`) gösterilir. Normatif tablo ve kurallar: **`docs/ELLIOTT_V2_STANDARDS.md` §8.1**.

---

## 1. Tespit Edilen Hatalar ve Sorunlar

### 1.1 KRİTİK — Çalışma Zamanı Riskleri

**H1: `position_manager.rs` — live gateway ömrü (iyileştirildi — FAZ 0.4)**
- Eski risk: her tick’te yeni gateway.
- **Durum:** live `(user_id, segment)` → paylaşımlı `Arc<BinanceLiveGateway>` önbelleği. Detay: FAZ **0.4** tablosu.

**H2: `kill_switch.rs` — halt sonrası geri alma (kapatıldı — FAZ 0.1)**
- Eski risk: yalnızca restart ile geri dönüş.
- **Durum:** `POST /api/v1/admin/kill-switch/reset` + worker DB senkronu; admin rolü. Detay: FAZ **0.1** tablosu.

**H3: `confluence.rs` — eksik veri ile nötr sinyalin ayrılmaması (kapatıldı — FAZ 0.3)**
- Eski risk: `fetch_data_snapshot` `None` dönerse bileşen 0.0 olarak hesaba katılıyordu; `confidence` eksik kaynakları yansıtmıyordu.
- **Durum:** Kullanılabilirlik çarpanı + `components_missing` / `data_availability`; detay **FAZ 0.3** tablosu.

**H4: `strategy_runner.rs` — 4 strateji aynı sanal bakiyeyi paylaşıyor (kapatıldı — FAZ 0.2)**
- Eski risk: tek `DryRunGateway` → paylaşılan sanal bakiye.
- **Durum:** `dry_gateway_for_strategy` + strateji/env bütçeleri; detay **FAZ 0.2** tablosu.

### 1.2 ORTA — Tasarım Sorunları

**M1: API hata dönüş tipi `Result<..., String>` tutarsız**
- Sorun: `ai_approval.rs`, `reconcile.rs`, `analysis.rs` vb. route handler'lar `Result<Json<T>, String>` dönüyordu. Axum bu durumda 500 ile düz metin gövde dönerdi.
- Etki: İstemci yapılandırılmış hata JSON'ı alamaz; hata kodu (400 vs 404 vs 500) ayrılmazdı.
- Çözüm: Ortak `ApiError` + `IntoResponse`, gövde `{"error": "..."}`. `/oauth/token` RFC 6749 gövdesi ayrı kalır.
- **Durum (2026-03):** Çözüldü — `crates/qtss-api/src/error.rs`, korumalı API route’ları `ApiError` kullanır.

**M2: `main.rs` (worker) — `SinkExt` ve WebSocket `send`**
- Durum: `futures_util::SinkExt`, `ws.send(Message::Pong(...))` çağrıları için **gereklidir**; trait import edilmezse derleme hata verir. “Kullanılmıyor” algısı, IDE’nin trait metodlarını import satırına bağlamamasından kaynaklanabilir.
- **Not:** `qtss-worker/src/main.rs` içinde import satırına kısa açıklayıcı yorum eklendi (`SinkExt` → WebSocket `.send`).

**M3: `web/nul` ve kök `nul` — Windows artifact**
- Sorun: Windows’ta yanlış çıktı yönlendirmesi `web/nul` veya repo kökünde `nul` dosyası oluşturabilir.
- Durum: `web/nul` repodan kaldırıldı; kök `.gitignore` içinde `web/nul` ignore ediliyor. Kökte `nul` kaldıysa WSL’de `rm -f nul` ile silin; git’e eklemeyin.

**M4: Exchange `"binance"` hardcoded — çoklu borsa genişlemesini zorlaştırır**
- Sorun: `main.rs` içinde kline için borsa sabitti. Kline WS loop'ları yalnız Binance protokolüyle uygulanır.
- **Durum (2026-03):** `QTSS_MARKET_DATA_EXCHANGE` + `system_config.worker.market_data_exchange` → `ExchangeId`; yalnızca `binance` birleşik kline WS’i başlatır; `bybit` / `okx` / `custom` için WS kapalı (yanlış `market_bars.exchange` etiketi önlenir). Tam adapter: **§2.3 madde 12**.

**M5: `ai_approval_requests` ile `ai_decisions` ilişkisi**
- İkisi ayrı tablolar kalır; bağlantı **`approval_request_id`** (`ai_decisions` → `ai_approval_requests`, `ON DELETE SET NULL`). Minimal zincir: `migrations/0013_ai_approval_requests_and_decision_engine.sql`. API: admin **`POST /api/v1/ai/decisions/{id}/link-approval-request`** (gövde: `approval_request_id`); LLM kararı onay/red veya otomatik onay sonrası kuyruk satırı **`sync_linked_approval_request_status`** ile eşitlenir (`qtss-ai` `storage` + `approval`).

### 1.3 DÜŞÜK — İyileştirme Fırsatları

**L1: Test coverage düşük (kısmen iyileştirildi)** — `qtss-ai`: `parser` / `safety` / `approval` / `context_builder` (`bar_ohlc_window_metrics`) birim testleri (`cargo test -p qtss-ai`). **Postgres entegrasyonları:** `decision_exists_for_hash_it`, `maybe_auto_approve_it` + CI `postgres-migrations` (`DATABASE_URL`). `qtss-storage` `config_tick`: `normalize_notify_locale_code` + JSON tick testleri. `qtss-worker`: `signal_scorer`, `confluence` yardımcıları; **`position_manager`** (`aggregate_long_books`, `intent_side`); **`strategy_runner`** (`strategy_env_suffix_normalized`). **İsteğe bağlı / ağır:** `position_manager` uçtan uca tick + AI + canlı DB senaryosu; `build_*_context` tam async yolu için ayrı DB fikstürü.

**L2: `pnl_rollup_loop` gecikme riski (iyileştirildi)** — Varsayılan tick **DB `system_config.worker.pnl_rollup_tick_secs`** (`{"secs":300}`) + `QTSS_PNL_ROLLUP_TICK_SECS` yedeği; `qtss_storage::resolve_worker_tick_secs` + `QTSS_CONFIG_ENV_OVERRIDES` önceliği (**FAZ 11**). Kill switch ile uyum için rollup sıklığı üretimde admin API veya env ile ayarlanmalıdır.

**L3: `migrations/README.md` envanter drift’i** — `migrations/README.md` bu checkout’taki dosyaları tablo ile listeler; tam ürün hattında **0001–0047** (+ **0048** sıradaki boş önek) olabilir. Drift: `ls migrations/*.sql | wc -l` ile README satır sayısı; §5 çift izlenebilirlik notu.

**L4: `docs/ELLIOTT_V2_STANDARDS.md` ↔ kod** — **§8 Implementation map** eklendi (`elliottEngineV2/*.ts`); kasıtlı değişikliklerde §2–§5 ile kod birlikte gözden geçirilmelidir.

### 1.4 Risk → FAZ eşlemesi (hızlı referans)

| Risk / konu | Ana başvuru | FAZ maddesi |
|---------------|-------------|-------------|
| Kill switch geri alma | H2 | 0.1 |
| Paylaşılan sanal bakiye | H4 | 0.2 |
| Confluence eksik veri / güven | H3 | 0.3 |
| Position manager gateway ömrü | H1 | 0.4 |
| `migrations/README.md` envanter | L3 | 0.5 (+ 1.7 AI sonrası) |
| API `String` hataları | M1 | 0.7 |
| AI DB şeması | M5, Bölüm 3 | 1.x |
| `web/nul` | M3 | 0.6 ✅ DONE |
| Test ağırlığı (worker çekirdeği) | L1 | Bölüm 7; **2.2** numaralı listede madde **9** (integration test) |
| PnL rollup vs kill switch gecikmesi | L2 | **2.2** numaralı listede madde **7** (PnL rollup sıklığı) |
| `exchange = "binance"` sabiti | M4 | Uzun vadeli öneri **12** (çoklu borsa adapter); ayrı FAZ satırı yok |
| Elliott doküman ↔ kod drift | L4 | `docs/ELLIOTT_V2_STANDARDS.md` **§8** (`elliottEngineV2/*.ts` eşlemesi) + §2–§5 ile kod senkronu |
| Çok dil (i18n) ürün hedefi | Bölüm 2.4 madde **13** | **FAZ 9** |
| Ortam değişkeni çoğalması / tek kaynak ihtiyacı | Bölüm 2.5 madde **14** | **FAZ 11** |

---

## 2. İyileştirme Önerileri

Bu bölümdeki öneriler **1–12** numaralı sürekli liste + **13** (çok dil) + **14** (merkezi DB yapılandırması) olarak düzenlenmiştir: **2.1** maddeler **1–5**, **2.2** maddeler **6–9**, **2.3** maddeler **10–12**, **2.4** madde **13** (detay **FAZ 9**), **2.5** madde **14** (detay **FAZ 11**). Bölüm **1.4** tablosundaki atıflar (ör. “2.2 numaralı listede madde 7”) bu ayrıma göredir.

### 2.1 Kısa Vadeli (Hemen)

1. **Kill switch reset endpoint** — **Tamamlandı (FAZ 0.1):** admin uç + worker DB senkronu.

2. **API error standardizasyonu** — **Tamamlandı (FAZ 0.7):** `ApiError` + JSON hata gövdesi.

3. **Strateji başına ayrı DryRunGateway** — **Tamamlandı (FAZ 0.2):** `dry_gateway_for_strategy` + env bütçeleri.

4. **Migrations README güncelle** — **tamamlandı** (**FAZ 0.5 + 1.7 ✅**): `migrations/README.md` güncel envanter + tam hatta **0048** sıradaki önek (§5).

5. **`web/nul`** — **tamamlandı** (M3, FAZ 0.6): repodan silindi, `.gitignore`’da `web/nul`.

### 2.2 Orta Vadeli (AI Engine öncesi)

6. **Confluence confidence skoru** — **Tamamlandı (FAZ 0.3):** kullanılabilirlik çarpanı + `components_missing` / `data_availability`.

7. **PnL rollup sıklığı** — **Tamamlandı:** `0045` tohum + `resolve_worker_tick_secs` (`pnl_rollup_loop`); varsayılan **300s** (5 dk), min **60s**; env `QTSS_PNL_ROLLUP_TICK_SECS`.

8. **Position manager gateway caching** — **Tamamlandı (FAZ 0.4):** live `(user_id, segment)` → `Arc<BinanceLiveGateway>`.

9. **Integration test altyapısı** — **Tamamlandı:** `.github/workflows/ci.yml` içinde **`postgres-migrations`** job’u (Postgres 16 servis + `DATABASE_URL`); `cargo test -p qtss-storage --test migrations_apply` — migrasyonlar + `system_config` `module = worker` seed sayısı (**0045**+**0046**).

### 2.3 Uzun Vadeli (AI Engine sonrası)

10. **Trailing stop desteği** — **Uygulandı:** `crates/qtss-worker/src/position_manager.rs` — `OrderType::TrailingStopMarket`, `QTSS_POSITION_MANAGER_TRAILING_ON_DIRECTIVE`, yönetilen futures stop-limit trailing; operasyonel **`activate_trailing`** / **`trailing_callback_pct`**, **`partial_close`** / **`full_close`** / **`deactivate_trailing`** (bkz. FAZ 5.3 modül başlığı).

11. **WebSocket fill stream** — **Uygulandı:** `crates/qtss-worker/src/binance_user_stream.rs` — USDM + spot user stream, fill `exchange_fills`, `exchange_orders` durum güncellemesi; `QTSS_BINANCE_USER_STREAM_ENABLED` ve `.env.example` blokları. Copy trade / reconcile bu kayıtlardan yararlanır; follower mantığı ayrı tick ile kalır.

12. **Çoklu borsa adapter** — `ExecutionGateway`: `BinanceLiveGateway`, `BybitLiveGateway` (linear **market** + **limit** + `/v5/order/cancel` `orderLinkId`), `OkxLiveGateway` (v5 **SWAP** **market** + **limit** / IOC / FOK / `post_only` + `/trade/cancel-order` `clOrdId`, `exchange_accounts.passphrase` zorunlu); API: `POST /api/v1/orders/bybit/place|cancel`, `POST /api/v1/orders/okx/place|cancel` (ops rolü); tamamlanmamış venue’ler `UnsupportedLiveGateway`; dry defter `instrument_position_key`; `position_manager` canlı kapatma: Binance + Bybit + OKX USDT futures (trailing/managed yolları hâlâ Binance; kline WS / reconcile çoğunlukla Binance).

> **Not (kapsam):** Maddeler **10–11** kodda karşılandı; **12** kademeli — spot/limit/cancel/WS ve `venue_order_id` uç durumları ayrı PR’lar.

### 2.4 Çok dil (i18n) — planlı ürün özelliği

13. **Uluslararasılaştırma (çok dil)** — Uygulama **çok dilli** olacaktır: web arayüzü, API’den dönen kullanıcıya yönelik metinler, bildirim şablonları (Telegram, e-posta, webhook) ve AI katmanı kullanıldığında operatör/raporlama diline uygun metin üretimi (hangi LLM sağlayıcısı seçilirse seçilsin). Teknik iş kırılımı ve durum takibi **FAZ 9** tablosunda; **FAZ 7** (web) ve **FAZ 5–6** (AI) ile koordine edilmelidir.

### 2.5 Merkezi yapılandırma (DB)

14. **`system_config` + `app_config` ayrımı** — İşletme ve ürün ayarlarının büyük kısmı `.env` yerine veritabanına taşınır; kök `.env` yalnızca **bootstrap** için minimal tutulur (aşağıda **FAZ 11**). Çoklu modül (`worker`, `api`, `notify`, `nansen`, `ai`, `execution`, …) için isim alanı, sırların env’de kalması veya aşamalı şifreleme, okuma önceliği (env override vs DB) ve admin API tam iş listesi **FAZ 11** tablosundadır.

---

## 3. AI Katmanı Entegrasyon Planı (çoklu sağlayıcı) — Güncel Durum

### 3.1 Mimari Felsefe

Mevcut sistem kural tabanlı skor matrisleri ile çalışıyor (`signal_scorer` → `onchain_signal_scorer` → `confluence`). AI bu sistemi **değiştirmez, güçlendirir:**

```
Mevcut:  Veri → Kural tabanlı skor → Emir
Hedef:   Veri → AI analizi (async, periyodik) → AI kararı (JSON) → DB
                                                                    ↓
         Veri → Kural tabanlı skor ────────────────────────────→ Emir (AI bilgisiyle zenginleştirilmiş)
```

AI **danışman** rolündedir: periyodik LLM çağrısı → yapılandırılmış JSON → DB. Yürütme katmanı bu JSON'u okur ama AI çökmüş olsa bile kural tabanlı modda çalışmaya devam eder.

### 3.1.1 Çoklu AI (sağlayıcı / uygulama çeşitliliği)

Ürün **yalnızca bir LLM veya tek bir harici uygulama** ile sınırlı değildir. `qtss-ai` içinde **ortak bir tamamlama arabirimi** (`async_trait` ile `complete(&AiRequest) -> Result<AiResponse>` gibi) tanımlanır; her gerçek uygulama **ayrı modül** (örn. `providers/anthropic.rs`, `providers/openai.rs`) veya ileride ayrı workspace üyesi crate olarak eklenebilir. **Katman başına** (taktik / operasyonel / stratejik) veya ileride görev başına farklı `provider_id`, taban URL ve API anahtarı `app_config` ve env ile seçilir; böylece örneğin taktik katman bir üreticide, stratejik katman başka bir üreticide veya kurum içi bir proxy’de çalışabilir. Yeni bir AI uygulaması eklemek, mevcut karar boru hattını bozmadan **yeni sağlayıcı implementasyonu + yapılandırma + (gerekirse) gizli anahtar** ile yapılır; `parser` / `safety` / DB şeması sağlayıcıdan bağımsız kalır.

**On-prem (kurum içi):** Bir veya daha fazla katman yalnızca **şirket ağındaki** çıkarım hizmetine bağlanabilir (`BASE_URL` → özel IP / iç DNS / air-gap köprüsü). Örnek yığınlar: **vLLM**, **Hugging Face TGI**, **Ollama**, **LM Studio** (API sunumu), **OpenAI uyumlu** kurumsal proxy. Bulut API anahtarı gerekmez; isteğe bağlı dahili bearer token, API key veya **mTLS** ile uç korunur. Worker’dan çıkan istekler yalnızca yapılandırılan iç uca gider — regülasyon ve veri ikameti gereksinimleri için planlı bir seçenektir. Uygulama işi **FAZ 2.3** + **FAZ 2.7** ile kapsanır.

### 3.2 Katman Mimarisi

Numaralandırma **üstten stratejik (4) → altta yürütme (1)** şeklindedir; günlük iş akışında en sık etkisi olması beklenen LLM katmanı **taktik (3)** ve **operasyonel (2)**’dir. Aşağıdaki örnek modeller **varsayılan plan** içindir; gerçek kurulumda **3.1.1** ile her katman farklı sağlayıcı + model ile eşleştirilebilir.

```
KATMAN 4: Stratejik AI (Günlük/Haftalık) — Claude Sonnet
  → Portföy hedefleri, risk bütçesi, rejim yorumu
  → ai_portfolio_directives (DB)
  
KATMAN 3: Taktik AI (15dk-1saat) — Claude Haiku ← EN KRİTİK
  → Sembol bazlı yön kararı, pozisyon büyüklüğü çarpanı
  → ai_tactical_decisions (DB)
  
KATMAN 2: Operasyonel AI (1-5dk) — Claude Haiku / yerel model
  → SL/TP güncelleme, trailing stop kararı
  → ai_position_directives (DB)
  
KATMAN 1: Yürütme (deterministic, AI'dan bağımsız) — Rust
  → position_manager, kill_switch, order_manager
  → AI kararlarını DB'den okur, bağımsız çalışabilir
```

### 3.3 Mevcut `ai_approval_requests` ile ilişki

Mevcut `ai_approval_requests` tablosu (migration 0038) genel amaçlı onay kuyruğu olarak kalacak. `ai_decisions` tablosu ayrı — ama onay gerektiren AI kararları `ai_approval_requests`'e de yazılarak operatör onayı alınabilir. Bu iki sistem birbirini tamamlar.

Uygulama sırası ve DB/API işleri bu belgede **FAZ 1–8** (AI), **FAZ 9** (çok dil), **FAZ 10** (bildirimler / **Telegram**) ve **FAZ 11** (`system_config` / `app_config` merkezi yapılandırma) tablolarında parçalanmıştır.

---

## 4. Cursor İçin Sıralı Görev Listesi

Aşağıdaki **FAZ 0–11** maddeleri **❌** / **✅ DONE** ile izlenir. **FAZ 0–8** (AI çekirdeği + API + web + env örnekleri), **FAZ 9** çekirdek i18n (web `react-i18next`, API locale, **`GET /api/v1/locales`**, `0045`, bildirim çift dili, CI `i18n:check`), **FAZ 10** ve **FAZ 11** çekirdeği tamamlandı. **FAZ 11.7** ana worker tick’leri `resolve_worker_tick_secs` ile bağlandı (tablo **11.7** + **CONFIG_REGISTRY**).

**Öncelik:** Üretim stabilitesi için **FAZ 0** (özellikle **0.1** kill switch, **0.2** bakiye, **0.3** confluence, **0.4** gateway) **FAZ 1–8** (AI) başlamadan tamamlanmalıdır; aksi halde LLM katmanı mevcut riskleri büyütür veya maskeler. **FAZ 9** (çok dil), **FAZ 0** sonrasında **FAZ 7** (web UI) ile paralel başlatılabilir; AI tarafında **9.5**, **FAZ 5–6** ile hizalanır. **FAZ 10** (Telegram ve olay bildirimleri) **AI’dan bağımsız** yürütülebilir; operasyonel değer için **FAZ 0** ile çakışmadan, `qtss-notify` + worker env’leri hazır oldukça erken devreye alınması önerilir. **FAZ 9.4** (bildirim şablonları / dil), **FAZ 10** metinleriyle koordine edilir. **FAZ 11** (DB yapılandırması) **AI ile seri değildir**; mevcut `app_config` kullanımını bozmamak için aşamalı taşıma ve çakışan migration numaraları (**Bölüm 5**) ile planlanmalıdır — **11.1** notuna bakın.

**Dosya yolu gösterimi:** Tablolarda `qtss-worker/src/...`, `qtss-api/src/...` gibi kısa yazımlar repo kökündeki `crates/qtss-worker/`, `crates/qtss-api/` ağacına karşılık gelir.

### FAZ 0 — Mevcut Hata Düzeltmeleri (AI öncesi temizlik)

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 0.1 | Kill switch reset endpoint | `routes/kill_switch_admin.rs`, `qtss-common/kill_switch.rs`, worker `kill_switch.rs` | `POST /api/v1/admin/kill-switch/reset` + `{"confirm":true}`; `app_config.kill_switch_trading_halted` + `resume_trading()`; worker DB sync (`QTSS_KILL_SWITCH_DB_SYNC_SECS`). | ✅ DONE |
| 0.2 | Strategy runner bakiye izolasyonu | `qtss-worker/src/strategy_runner.rs` | `dry_gateway_for_strategy`; `QTSS_STRATEGY_<NAME>_BALANCE` veya toplam/4. | ✅ DONE |
| 0.3 | Confluence confidence düşürme | `qtss-worker/src/confluence.rs` | Snapshot kullanılabilirliği ile `confidence` çarpanı; `components_missing`, `data_availability`. | ✅ DONE |
| 0.4 | Position manager gateway caching | `qtss-worker/src/position_manager.rs` | Live: `(user_id, segment)` → `Arc<BinanceLiveGateway>`. | ✅ DONE |
| 0.5 | Migrations README güncelle | `migrations/README.md` | Checkout envanteri tabloda; tam hatta son önek **0047** / sıradaki **0048** — §5. | ✅ DONE |
| 0.6 | `web/nul` sil + ignore | `web/nul`, `.gitignore` | Repo’dan kaldırıldı; `.gitignore`’da `web/nul`. | ✅ DONE |
| 0.7 | API error standardizasyonu | `qtss-api/src/error.rs`, route handler'lar | `ApiError` + `IntoResponse`; JSON `{"error": "..."}`; `From<sqlx::Error>` / `From<StorageError>`. OAuth `/oauth/token` ayrı gövde (RFC 6749). | ✅ DONE |

### FAZ 1 — AI Engine Veritabanı Altyapısı

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 1.1 | Migration: `0042_ai_engine_tables.sql` | `migrations/0042_ai_engine_tables.sql` | `ai_decisions` + indeksler (`symbol,layer,created_at`; partial `status`). | ✅ DONE |
| 1.2 | `ai_tactical_decisions` | 0042 içinde | FK `decision_id`, direction/status CHECK, indeks `(symbol, status, created_at DESC)`. | ✅ DONE |
| 1.3 | `ai_position_directives` | 0042 içinde | action/status CHECK, sembol + decision indeksleri. | ✅ DONE |
| 1.4 | `ai_portfolio_directives` | 0042 içinde | `symbol_scores` JSONB, `status` default `active`. | ✅ DONE |
| 1.5 | `ai_decision_outcomes` | 0042 içinde | `outcome` CHECK; `recorded_at`. | ✅ DONE |
| 1.6 | Migration: `0043_ai_engine_config.sql` | `migrations/0043_ai_engine_config.sql` | `app_config` `ai_engine_config` seed; `ON CONFLICT (key) DO NOTHING`. | ✅ DONE |
| 1.7 | `migrations/README.md` güncelle | `migrations/README.md` | Bu ağaç + tam ürün hattı notu; sıradaki boş önek **0048**. | ✅ DONE |

### FAZ 2 — `qtss-ai` Crate İskeleti

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 2.1 | Crate oluştur + workspace'e ekle | `crates/qtss-ai/Cargo.toml`, kök `Cargo.toml` | `[package] name = "qtss-ai"`. Dependencies: tokio, tracing, serde, serde_json, chrono, uuid, anyhow, thiserror, async-trait, sqlx, reqwest (`features = ["json", "rustls-tls"]`), sha2, hex + workspace üyeleri (qtss-common, qtss-domain, qtss-storage, qtss-notify). Kök Cargo.toml'a `"crates/qtss-ai"` member + `qtss-ai = { path = "crates/qtss-ai" }` ekle. | ✅ DONE |
| 2.2 | `src/lib.rs` — modül tanımları | `crates/qtss-ai/src/lib.rs` | `pub mod providers; pub mod client; pub mod context_builder; pub mod parser; pub mod layers; pub mod storage; pub mod approval; pub mod safety;` Re-export: `pub use client::AiRuntime;` (veya eşdeğer isim) — runtime, `app_config` / env’den **katman başına** doğru `dyn AiCompletionProvider` veya enum ile seçilen sağlayıcıyı tutar. | ✅ DONE |
| 2.3 | `providers/` + `client.rs` — çoklu sağlayıcı (bulut + on-prem) | `crates/qtss-ai/src/providers/mod.rs`, `anthropic.rs`, `client.rs` | **`providers/mod.rs`:** trait `AiCompletionProvider`: `async fn complete(&self, req: &AiRequest) -> Result<AiResponse>`. Ortak `AiRequest` / `AiResponse` tipleri. **`anthropic.rs`:** ilk bulut referansı — `AnthropicProvider::from_env()` → `ANTHROPIC_API_KEY` + `ANTHROPIC_BASE_URL` (varsayılan `https://api.anthropic.com`). HTTP: `POST {base_url}/v1/messages`, headers: `x-api-key`, `anthropic-version: …` ([Anthropic Messages](https://docs.anthropic.com/en/api/messages)). Timeout: 120s; hata log’unda ilk 500 karakter. **On-prem uygulamalar:** aynı trait ile `OpenAiCompatibleProvider` (iç `BASE_URL` + `/v1/chat/completions` veya kurum şeması), `OllamaProvider` vb.; ham HTTP farkı `complete` içinde kalır. **`client.rs`:** `AiRuntime::tactical_provider()`, `operational_provider()`, `strategic_provider()` — `ai_engine_config` içindeki `provider_*` + model alanlarına göre doğru struct’ı üretir; bilinmeyen `provider_id` için açık hata. İleride `openai.rs`, `openai_compatible_onprem.rs`, `ollama.rs` vb. eklenir. **`Clone` / `Arc`** — worker’da katman spawn’ları paylaşımlı veya ayrı örnek kullanabilir. Operasyonel on-prem maddeleri **FAZ 2.7**. | ✅ DONE |
| 2.4 | `src/storage.rs` — AI tablo DB fonksiyonları | `crates/qtss-ai/src/storage.rs` | `insert_ai_decision(pool, layer, symbol, model_id, prompt_hash, input_snapshot, raw_output, parsed_decision, confidence) -> Result<Uuid>`. `insert_tactical_decision(pool, decision_id, symbol, parsed, valid_until) -> Result<Uuid>`. `insert_position_directive(pool, ...)`. `insert_portfolio_directive(pool, ...)`. `fetch_latest_approved_tactical(pool, symbol) -> Option<Row>`. `fetch_latest_approved_directive(pool, symbol) -> Option<Row>`. `mark_applied(pool, table, id)`. `expire_stale_decisions(pool)` — `status='pending_approval' AND expires_at < now()` → `status='expired'`. `decision_exists_for_hash(pool, hash, ttl_minutes) -> bool`. | ✅ DONE |
| 2.5 | `src/parser.rs` — LLM JSON ayrıştırıcı | `crates/qtss-ai/src/parser.rs` | `parse_tactical_decision(raw: &str) -> Result<Value>`: JSON blok çıkarma (```json...``` veya ham {}), `direction` zorunlu (strong_buy/buy/neutral/sell/strong_sell/no_trade), `confidence` zorunlu (0.0-1.0), `position_size_multiplier` sınır (0.0-2.0). `parse_operational_decision(raw) -> Result<Value>`: `action` zorunlu (keep/tighten_stop/widen_stop/activate_trailing/...). `extract_json_block(raw) -> String`: yardımcı. **Birim testleri:** Her parse fonksiyonu için en az 3 test (geçerli, geçersiz direction, eksik alan). | ✅ DONE |
| 2.6 | `src/safety.rs` — güvenlik doğrulama | `crates/qtss-ai/src/safety.rs` | `validate_ai_decision_safety(decision: &Value, config: &SafetyConfig) -> Result<(), &'static str>`: (1) `position_size_multiplier <= config.max_size_multiplier`, (2) `stop_loss_pct` zorunlu (buy/sell kararlarında), (3) `qtss_common::is_trading_halted()` kontrolü. `SafetyConfig`: `max_size_multiplier` (env `QTSS_AI_MAX_POSITION_SIZE_MULT`, varsayılan 1.5). | ✅ DONE |
| 2.7 | On-prem inference operasyonu ve güvenli bağlantı | `crates/qtss-ai/src/providers/`, `.env.example`, iç doc | **Hedef:** QTSS worker’ın bağlam/prompt verisini **yalnızca** tanımlı kurum içi uca göndermesi (bulut sırları olmadan çalışma seçeneği). **Yapılandırma:** dahili `BASE_URL(ler)`, model adı (örn. vLLM’de kayıtlı model), isteğe bağlı `Authorization` / özel header, **mTLS** veya kurum CA ile `reqwest` TLS. **Operasyon:** on-prem için ayrı timeout ve eşzamanlı istek üst sınırı (örn. `QTSS_AI_ONPREM_TIMEOUT_SECS`, `QTSS_AI_ONPREM_MAX_IN_FLIGHT` — isimler **FAZ 8.1** ile netleşir). **Gözlemlenebilirlik:** `meta_json.provider` + iç endpoint host (PII olmadan) audit için yeterli olmalıdır. Air-gap ortamlarda DNS/TLS ve çıkarım kümesi kullanılabilirliği runbook ile dokümante edilir. **FAZ 2.3** ile aynı `AiCompletionProvider` sözleşmesi. | ✅ DONE |

### FAZ 3 — Context Builder (DB → LLM Bağlamı)

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 3.1 | `src/context_builder.rs` — taktik bağlam | `crates/qtss-ai/src/context_builder.rs` | `pub async fn build_tactical_context(pool, symbol) -> Result<Value>`: (1) `fetch_latest_onchain_signal_score(pool, symbol)` → aggregate_score, confidence, direction, conflict_detected, funding_score, nansen_sm_score, (2) `fetch_analysis_snapshot(pool, symbol, "confluence")` → composite_score, regime, pillar_scores, (3) `market_bars` son 20 mum → son fiyat, 24h değişim %, volatilite (high-low range / close ortalaması), (4) `exchange_orders` açık pozisyon özeti (entry, size, side, unrealized_pnl_pct), (5) Son AI kararı (24h içi, tekrar aynı kararı vermemek için). Çıktı: `{"symbol", "timestamp_utc", "onchain_signals", "confluence", "price_context", "open_position", "last_ai_decision"}`. **Token bütçesi:** ~2000 token; ham bar yerine istatistik özeti. | ✅ DONE |
| 3.2 | `context_builder.rs` — operasyonel bağlam | Aynı dosya | `pub async fn build_operational_context(pool, symbol) -> Result<Value>`: Sadece açık pozisyon varsa çalışır. Açık pozisyon özeti + son 5 mum + funding snapshot + onchain özet (aggregate_score, direction, conflict_detected). ~1000 token. | ✅ DONE |
| 3.3 | `context_builder.rs` — stratejik bağlam | Aynı dosya | `pub async fn build_strategic_context(pool) -> Result<Value>`: Tüm sembollerin son confluence skorları + 7 günlük PnL özeti + portföy maruz kalma. ~8000 token. | ✅ DONE |
| 3.4 | `qtss-storage` — eksik yardımcı fonksiyonlar | `crates/qtss-storage/src/` | Eğer eksikse ekle: `fetch_latest_onchain_signal_score(pool, symbol) -> Option<OnchainSignalScoreRow>`, `fetch_open_positions_summary(pool, symbol)` (exchange_orders dolmuş ama kapanmamış net long), `fetch_recent_bars_stats(pool, symbol, n)` (son n mum istatistiği). Bu fonksiyonlar `context_builder`'ın DB okumasını sağlar. | ✅ DONE |

### FAZ 4 — Taktik AI Katmanı (En Kritik)

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 4.1 | `src/layers/mod.rs` | `crates/qtss-ai/src/layers/mod.rs` | `pub mod tactical; pub mod operational; pub mod strategic;` | ✅ DONE |
| 4.2 | `src/layers/tactical.rs` — taktik karar döngüsü | `crates/qtss-ai/src/layers/tactical.rs` | `TacticalLayer { pool, tactical_provider: Arc<dyn AiCompletionProvider + Send + Sync> }` (veya enum) + `pub async fn run(self)`. Tick: `QTSS_AI_TACTICAL_TICK_SECS` (varsayılan 900). Her tick: (1) `ai_engine_enabled` kontrolü (app_config'den), (2) `list_enabled_engine_symbols`, (3) Her sembol için: `build_tactical_context` → `hash_context` (SHA-256) → `decision_exists_for_hash` (30dk TTL) kontrolü → **`tactical_provider.complete(...)`** → `parse_tactical_decision` → safety validation → `insert_ai_decision` + `insert_tactical_decision` → `maybe_auto_approve`. `ai_decisions.model_id` / `meta_json.provider` ile hangi sağlayıcının kullanıldığı kaydedilir. Sistem promptu: JSON-only, direction/confidence/stop_loss_pct zorunlu, `no_trade` geçerli, `temperature: 0.3`. Hata durumunda `insert_ai_decision_error` (status='error'). `no_trade` kararı DB'ye yazılmaz, sadece log. Minimum confidence (app_config `require_min_confidence`, varsayılan 0.60) altı → skip. | ✅ DONE |
| 4.3 | `src/approval.rs` — otomatik onay | `crates/qtss-ai/src/approval.rs` | `maybe_auto_approve(pool, decision_id, confidence)`: `QTSS_AI_AUTO_APPROVE_ENABLED=1` VE `confidence >= threshold` → `ai_decisions.status='approved'` + `ai_tactical_decisions.status='approved'`. Değilse: `qtss-notify` ile Telegram/webhook bildirim (sembol, direction, confidence, reasoning). | ✅ DONE |
| 4.4 | `src/layers/tactical.rs` — sistem promptu | Aynı dosya | Türkçe reasoning, JSON-only, karar kriterleri: `aggregate_score > 0.6 AND !conflict → buy/strong_buy`, `< -0.6 AND !conflict → sell/strong_sell`, `conflict → multiplier 0.5 veya no_trade`, `zaten açık pozisyon + aynı yön → no_trade`. `confidence < 0.5 → no_trade`. | ✅ DONE |

### FAZ 5 — Worker Entegrasyonu

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 5.1 | `qtss-worker` Cargo.toml'a `qtss-ai` ekle | `crates/qtss-worker/Cargo.toml` | `qtss-ai.workspace = true` | ✅ DONE |
| 5.2 | `main.rs`'e AI spawn'ları ekle | `crates/qtss-worker/src/main.rs` | DATABASE_URL bloğu sonunda: `AiRuntime::from_env_and_config(pool.clone())` (veya eşdeğer) ile **katman başına** sağlayıcı örnekleri oluştur; **bulut** sağlayıcıda gerekli API anahtarı yoksa veya **on-prem** uca erişim kurulamadıysa ilgili katman atlanır veya tüm AI kapalı kalır. Örnek: `if let Ok(runtime) = AiRuntime::load(...) { tokio::spawn(tactical_layer.run(runtime.tactical())); tokio::spawn(operational::run(runtime.operational())); if strategic_enabled { tokio::spawn(strategic::run(runtime.strategic())); } } else { warn!("AI sağlayıcı yapılandırması eksik — AI katmanı kapalı"); }`. Ana döngüde ek: `tokio::spawn(qtss_ai::storage::expire_stale_decisions_loop(pool))` — note: `expire_stale_ai_decisions_loop` in `qtss-ai/src/lib.rs`, `ai_engine.rs` spawns layers + expiry. | ✅ DONE |
| 5.3 | `position_manager.rs`'de AI kararlarını oku | `crates/qtss-worker/src/position_manager.rs` | Her tick'te (mevcut SL/TP kontrolünden ÖNCE): (1) `SELECT * FROM ai_tactical_decisions WHERE symbol=$1 AND status='approved' AND valid_until > now() ORDER BY created_at DESC LIMIT 1`. Varsa: `effective_sl = td.stop_loss_pct.unwrap_or(default_sl)`, `effective_tp = td.take_profit_pct.unwrap_or(default_tp)`, `effective_multiplier = td.position_size_multiplier.clamp(0.0, 2.0)`. Uygulandıktan sonra: `UPDATE ai_tactical_decisions SET status='applied'`. (2) `SELECT * FROM ai_position_directives WHERE symbol=$1 AND status='approved' AND created_at > now() - interval '10 min' ORDER BY created_at DESC LIMIT 1`. Varsa: `match action { "tighten_stop" => ..., "activate_trailing" => ..., "partial_close" => ..., "full_close" => ... }`. **AI yoksa:** Mevcut kural tabanlı mantık aynen çalışır — geriye uyumluluk korunur. | ✅ DONE |

### FAZ 6 — Operasyonel ve Stratejik Katmanlar

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 6.1 | `src/layers/operational.rs` | `crates/qtss-ai/src/layers/operational.rs` | 2dk tick. Yalnızca açık pozisyon olan semboller için çalışır. `build_operational_context` → LLM → `parse_operational_decision` → `insert_position_directive` → `maybe_auto_approve`. Sistem promptu: trailing stop kararı, stop güncelleme (kötüleştirilemez), partial/full close. | ✅ DONE |
| 6.2 | `src/layers/strategic.rs` | `crates/qtss-ai/src/layers/strategic.rs` | Günde 1 (86400s). `build_strategic_context` → büyük model (Sonnet) → `insert_portfolio_directive`. Çıktı: risk_budget_pct, max_open_positions, preferred_regime, symbol_scores. Taktik katman bu direktifleri okuyarak sembol ağırlıklarını ayarlar. `QTSS_AI_STRATEGIC_ENABLED=1` ile açılır. | ✅ DONE |
| 6.3 | Öğrenme döngüsü (feedback) | `crates/qtss-ai/src/feedback.rs` | Pozisyon kapandığında `ai_decision_outcomes`'a kayıt. Stratejik katman son 30 kararın win_rate, avg_pnl, best_regime istatistiğini bağlama dahil eder. Gerçek ML training yok — LLM geçmiş performansı bağlamdan okur. | ✅ DONE |

### FAZ 7 — API Endpoints + Web UI

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 7.1 | AI karar API'leri | `crates/qtss-api/src/routes/ai_decisions.rs` | `GET /api/v1/ai/decisions?layer=&symbol=&status=&limit=` — tüm roller okuyabilir. `GET /api/v1/ai/decisions/{id}` — detay. `POST /api/v1/ai/decisions/{id}/approve` — admin. `POST /api/v1/ai/decisions/{id}/reject` — admin. `GET /api/v1/ai/directives/tactical?symbol=` — son onaylı taktik karar. `GET /api/v1/ai/directives/portfolio` — aktif portföy direktifi. | ✅ DONE |
| 7.2 | Web UI: AI kararları paneli | `web/src/components/AiDecisionsPanel.tsx` | Taktik kararlar listesi (sembol, direction, confidence, status, reasoning). Pending kararları onaylama/reddetme butonları (admin). Son portföy direktifi kartı. | ✅ DONE |

### FAZ 8 — Ortam Değişkenleri + .env.example

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 8.1 | `.env.example`'a AI env'leri ekle | `.env.example` | Aşağıdaki blok: `# === AI (qtss-ai, çoklu sağlayıcı + on-prem) ===`, `ANTHROPIC_API_KEY=`, `# ANTHROPIC_BASE_URL=https://api.anthropic.com`, ikinci sağlayıcı için yorumlu örnek: `# OPENAI_API_KEY=`, `# OPENAI_BASE_URL=...` (implementasyon eklendikçe), **on-prem örnekleri (yorumlu):** `# QTSS_AI_OPENAI_COMPAT_BASE_URL=http://vllm.internal:8000/v1`, `# QTSS_AI_OLLAMA_BASE_URL=http://ollama.internal:11434`, `# QTSS_AI_ONPREM_API_KEY=` (iç gateway token; zorunlu değil), `# QTSS_AI_ONPREM_TIMEOUT_SECS=180`, `# QTSS_AI_PROVIDER_TACTICAL=anthropic` (veya `openai_compatible_onprem` / `ollama` — `app_config` öncelikli), model/tick/TTL değişkenleri: `# QTSS_AI_MODEL_TACTICAL=...`, …, `# QTSS_AI_TACTICAL_TICK_SECS=900`, `# QTSS_AI_OPERATIONAL_TICK_SECS=120`, `# QTSS_AI_STRATEGIC_TICK_SECS=86400`, `# QTSS_AI_AUTO_APPROVE_ENABLED=0`, `# QTSS_AI_AUTO_APPROVE_THRESHOLD=0.85`, `# QTSS_AI_MIN_CONFIDENCE=0.60`, `# QTSS_AI_STRATEGIC_ENABLED=0`, `# QTSS_AI_MAX_POSITION_SIZE_MULT=1.5`, `# QTSS_AI_DECISION_TTL_SECS=1800` | ✅ DONE |

### FAZ 9 — Çok dil (i18n / l10n)

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 9.1 | Web arayüzü i18n | `web/` | `web/src/i18n.ts`, `locales/en.json` + `tr.json`, **`locales/supportedLocales.ts`**, `LanguageSwitcher`; `App.tsx`: `app.channelReject` / `app.channelScan`, `app.chartToolbar`, `app.acp`, `app.engineDrawer` (motor özeti, snapshot/Confluence boş durumları), **`app.signalDashboard`** (DB sinyal paneli tablosu + wire özeti), **`app.paperDrawer`** (F4/F5 Range–Paper–komisyon kartı), `app.drawerPanel`, `app.backfill` / `app.bars` / `app.commission` / `app.config` / `app.dev` / `app.drawings`; Elliott/bağlam uzun metinler; **`apiErrors.*`** (FAZ 9.2) ayrı ağaç. | ✅ DONE (çekirdek) |
| 9.2 | API kullanıcı mesajları | `crates/qtss-api/src/`, `web/` | `locale.rs`: `Accept-Language` + `?locale=`; `X-QTSS-Negotiated-Locale`; `ApiError`: `error`, `locale`, **`error_key`**, **`error_args`**. **`routes/session.rs`**: `with_error_key` ör. `session.invalid_sub`, `session.locale_invalid_*`. **Public katalog:** `GET /api/v1/locales` — `fetchSupportedLocales`. **Web:** `web/src/lib/apiErrorFormat.ts` (`formatQtssApiErrorMessage`, `throwQtssApiError`); `web/src/api/client.ts` korumalı uçlar OAuth gövdesi dahil bu yardımcıyı kullanır; `locales/*` içinde **`apiErrors.session`** + **`apiErrors.common`** (genişletilebilir başlangıç). | ✅ DONE |
| 9.3 | Kullanıcı locale tercihi | `migrations/0045_…`, `qtss-storage/users.rs`, `routes/session.rs`, `web` | `users.preferred_locale`; `GET /api/v1/me` (`preferred_locale`) + `PATCH /api/v1/me/locale`; web `patchMePreferredLocale` / `LanguageSwitcher`. | ✅ DONE |
| 9.4 | Bildirim şablonları | `crates/qtss-notify/src/locale.rs`, worker `paper_fill_notify`, `live_position_notify` | `resolve_bilingual`; worker varsayılan dil `resolve_notify_default_locale` + `worker.notify_default_locale` / `QTSS_NOTIFY_DEFAULT_LOCALE`. | ✅ DONE (paper/live özet) |
| 9.5 | AI dil politikası (sağlayıcıdan bağımsız) | `crates/qtss-ai/`, `app_config` | Sistem promptu ve `reasoning` çıktısı için hedef dil (örn. `QTSS_AI_OUTPUT_LOCALE` veya `ai_engine_config` alanı); tüm `AiCompletionProvider` implementasyonları aynı locale kuralına uyar; **FAZ 4–6** ile uyumlu. | ✅ DONE (prompt kökü `AiEngineConfig.output_locale` + `QTSS_AI_OUTPUT_LOCALE`) |
| 9.6 | Test ve CI | `.github/workflows/ci.yml`, `web/scripts/check-i18n-keys.mjs`, kök `scripts/check-i18n-keys.mjs` | Yerel: `cd web && npm run i18n:check` veya repo kökünden `node scripts/check-i18n-keys.mjs`. CI: `npm run i18n:check` + `npm run build`; Rust (`qtss-worker --bin qtss-worker`); **`postgres-migrations`**: `migrations_apply` + **`qtss-ai` `decision_exists_for_hash_it`** + **`maybe_auto_approve_it`** (**§2.2 madde 9**, **§7**). | ✅ DONE |

### FAZ 10 — Bildirim servisleri: Telegram etkinleştirme ve olay akışları

**Hedef kanallar (`qtss-notify`):** Bildirim altyapısında **Telegram** (Bot API), **webhook**, **Discord** ve kodda desteklenen diğer kanallar yer alır; **Telegram** operatör uyarıları için birinci sınıf hedeftir ve aşağıdaki aksiyonlarla **aktif** hale getirilir. Kesin env adları ve varsayılanlar kök **`.env.example`** (qtss-notify / worker blokları) ve `crates/qtss-notify` yapılandırması ile doğrulanmalıdır.

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 10.1 | Telegram kimlik bilgisi + uçtan uca test | `.env`, `qtss-notify`, `qtss-api` | `QTSS_NOTIFY_TELEGRAM_BOT_TOKEN`, `QTSS_NOTIFY_TELEGRAM_CHAT_ID` (veya crate’teki kesin isimler) tanımlanır. `POST /api/v1/notify/test` gövdesinde `channel` = `telegram` ile smoke; başarılı gönderim runbook’a işlenir (BotFather, sohbet/kanal ID). | ✅ DONE |
| 10.2 | `notify_outbox` + Telegram | worker, `qtss-notify` | `QTSS_NOTIFY_OUTBOX_ENABLED=1`, `QTSS_NOTIFY_OUTBOX_TICK_SECS` — kuyruktaki kayıtların `channels` listesinde `telegram` olduğunda worker’ın `qtss-notify` ile gerçekten Telegram’a ilettiğini doğrula; hata/retry log’ları operatör için okunur olmalı. | ✅ DONE |
| 10.3 | **Sinyal** bildirimleri (Telegram) | worker | On-chain eşik: `QTSS_NOTIFY_ON_ONCHAIN_SIGNAL` + kanal env’leri; **takma ad** `QTSS_NOTIFY_SIGNAL_ENABLED` / `QTSS_NOTIFY_SIGNAL_CHANNELS` (`onchain_signal_scorer.rs`). Sweep: `QTSS_NOTIFY_ON_SWEEP_*`. | ✅ DONE |
| 10.4 | **Setup** bildirimleri (Telegram) | worker (setup scan) | `QTSS_NOTIFY_SETUP_*` env’leri + setup döngüsü; çok dil şablonu **FAZ 9.4** ile genişletilebilir. | ✅ DONE |
| 10.5 | **Alım / satım** bildirimleri (Telegram) | worker | Paper: `paper_fill_notify`; live: `live_position_notify`; ilgili `QTSS_NOTIFY_*` env’leri. | ✅ DONE |
| 10.6 | Dokümantasyon ve `.env.example` birliği | `.env.example`, bu rehber, isteğe bağlı `docs/SECURITY.md` | `SECURITY.md` Telegram checklist; `.env.example` kanal örnekleri. | ✅ DONE |

### FAZ 11 — Merkezi yapılandırma: `system_config` + `app_config`

**Hedef:** Üretimde `.env` dosyasını **bootstrap** düzeyinde tutmak: tipik olarak yalnızca **PostgreSQL bağlantısı** (`DATABASE_URL`) ve uygulamanın DB’ye hiç ulaşamadan önce gereken **çok küçük** bir set (aşağıda **11.9**). Worker / API / bildirim / Nansen / AI / execution gibi **modüllerin** ayarları `system_config` ve mevcut **`app_config`** üzerinden yönetilir; böylece çok modüllü büyüme için tek şema ve isim alanı disiplini sağlanır.

**İki tablo — sorumluluk ayrımı**

| Tablo | Amaç | Örnek içerik |
|-------|------|----------------|
| **`system_config`** (yeni) | Süreç / platform / operasyon parametreleri: tick süreleri, döngü `*_ENABLED` bayrakları, iç URL’ler, eşikler (sırlı olmayan), modül başına gruplama | `worker.notify_outbox`, `worker.kill_switch`, `api.rate_limit`, `notify.telegram` (yalnızca *hassas olmayan* alanlar; token değil) |
| **`app_config`** (mevcut, `0001_init`) | Ürün / analiz / strateji JSON blob’ları; admin UI ile bugün de kullanılıyor | `confluence_weights_by_regime`, `acp_chart_patterns`, ileride `ai_engine_config` (**FAZ 1.6**) |

**Çok modül için şema (önerilen `system_config` sütunları)**

- `id` UUID PK, `module` TEXT NOT NULL — mantıksal sahip: `worker`, `api`, `notify`, `nansen`, `ai`, `execution`, `oauth`, `metrics`, … (yeni modül = yeni `module` değeri; kod incelemesinde enum veya sabit liste ile doğrulanabilir).
- `config_key` TEXT NOT NULL — modül içi benzersiz anahtar (`snake_case`); ör. `outbox_tick_secs`, `feature_notify_setup`.
- `value` JSONB NOT NULL — skalar yerine nesne tercih edilir: `{"tick_secs":10,"enabled":true}`; `schema_version` ile uyumlu evrim.
- `schema_version` INT NOT NULL DEFAULT 1 — aynı `config_key` için JSON şekil değişimini migrate eden job’lar için.
- `description` TEXT, `is_secret` BOOLEAN NOT NULL DEFAULT false — `true` ise list API’de maskeleme; değer asla düz metin loglanmaz (yine de **11.9**: aşama 1’de sırlar tercihen env / secret store).
- `updated_at` TIMESTAMPTZ, `updated_by_user_id` UUID NULL REFERENCES `users` — audit ile uyumlu.
- **Kısıt:** `UNIQUE(module, config_key)`; indeks: `(module)` veya `(module, config_key)` arama için.

**`app_config` genişletme (isteğe bağlı migration)**

- `module` TEXT NULL + indeks — mevcut satırlar `NULL`; yeni satırlar `chart`, `confluence`, `ai`, `i18n` gibi doldurur. Alternatif: sadece `key` içinde `module.subsystem` ön eki konvansiyonu (dokümante edilir); ikisi birden kullanılmamalı — **11.2** bir yolu seçer.
- **İleride (FAZ 11 dışı not):** çok kiracı için `org_id` UUID NULL; `UNIQUE(org_id, key)` ile org-özel override — şimdilik tek org varsayımıyla planlanır.

**Çözümleme önceliği (runtime)**

1. **Bootstrap:** `DATABASE_URL` (zorunlu); isteğe bağlı `RUST_LOG`, `SQLX_OFFLINE` (CI), tek seferlik migrasyon bayrakları.
2. **Override:** Tanımlı `QTSS_*` env değişkeni varsa ve `QTSS_CONFIG_ENV_OVERRIDES=1` (veya anahtar bazlı allowlist) ise env kazanır — felaket kurtarma ve CI için.
3. **Aksi halde:** `system_config` / `app_config` değeri (RAM önbellek + TTL veya değişiklikte invalidate).

**Sırlar stratejisi (aşamalı)**

- **Aşama A:** `JWT_SECRET`, borsa anahtarları, bot token’lar, LLM API key’leri **`.env` / secret store**’da kalır; `system_config.is_secret` satırları yalnızca *referans* (örn. Vault path) veya boş.
- **Aşama B:** Uygulama içi şifreleme + KMS ile DB’de saklama (ayrı proje).

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 11.1 | Migration: `system_config` | `migrations/0044_system_config.sql` | DDL + idempotent seed (`ai.worker_doc`). `migrations/README.md` güncel. | ✅ DONE |
| 11.2 | `app_config` modül alanı (veya konvansiyon) | `docs/CONFIG_REGISTRY.md` | `app_config` için ayrı `module` kolonu **eklenmedi**; anahtar ön ekleri ve `system_config.module` ayrımı dokümante. | ✅ DONE (konvansiyon) |
| 11.3 | İsim alanı ve kayıt defteri | `docs/CONFIG_REGISTRY.md` | Modül listesi, admin API özeti, PR checklist. | ✅ DONE |
| 11.4 | `qtss-storage` | `crates/qtss-storage/src/system_config.rs` | `SystemConfigRepository`: list/get/upsert/delete; `is_secret` listelerde maskeleme. | ✅ DONE |
| 11.5 | Çözümleme katmanı | `qtss-common/src/config_resolve.rs` | `QTSS_CONFIG_ENV_OVERRIDES` + `env_override`; tam DB birleşik çözüm aşamalı. | ✅ DONE (ince katman) |
| 11.6 | Admin API | `qtss-api` | `GET/POST/DELETE /api/v1/admin/system-config` (`routes/system_config_admin.rs`); list `?module=`. **admin** rolü. | ✅ DONE |
| 11.7 | Modül bazlı taşıma (incremental) | `qtss-worker` (`nansen_engine`, `nansen_extended`, `onchain_signal_scorer`, `position_manager`, `engines/external_common`, …), `qtss-analysis/src/engine_loop.rs`, `qtss-storage/src/config_tick.rs`, `qtss-ai/src/lib.rs`, `0045…`–`0047` (+ isteğe bağlı **0048+** `external_fetch_poll_tick_secs` tohumu) | **✅ çekirdek taşıma:** yukarıdaki bileşenlerde `resolve_worker_tick_secs` + env yedekleri (**`docs/CONFIG_REGISTRY.md`**). Harici `external_data_sources` aile döngüleri: **`worker.external_fetch_poll_tick_secs`** / **`QTSS_EXTERNAL_FETCH_POLL_SECS`**. `normalize_notify_locale_code` testleri. **İsteğe bağlı:** ek `system_config` anahtarları için idempotent migration. | ✅ DONE (çekirdek) |
| 11.8 | Seed / import | `0044`–`0047` | Idempotent `ON CONFLICT DO NOTHING` seed. | ✅ DONE |
| 11.9 | `.env` minimal politika | `.env.example`, `SECURITY.md`, **Bölüm 6** | Bootstrap: `DATABASE_URL`, `QTSS_JWT_SECRET`; `QTSS_CONFIG_ENV_OVERRIDES` notu. | ✅ DONE |
| 11.10 | Test + gözlemlenebilirlik | `cargo test`, `migrations_apply`, `qtss-common/src/config_resolve.rs` | `config_tick` (`tick_secs_from_config_value`, `normalize_notify_locale_code`) + `qtss-notify` `locale`; `qtss-common` **`config_resolve`** + **`kill_switch`**; **`qtss-ai`** parser/safety/approval/context metrik testleri (**Bölüm 7**); **CI:** `migrations_apply` (Postgres) + **`qtss-ai` `decision_exists_for_hash_it`** + **`maybe_auto_approve_it`** + **`qtss-worker --bin qtss-worker`**. | ✅ çekirdek (lib + `qtss-ai` Postgres entegrasyonları) |

**FAZ 0–11 üst seviye özet**

| FAZ | Kapsam | Durum |
|-----|--------|--------|
| 0 | AI öncesi temizlik (kill switch reset, bakiye izolasyonu, confluence confidence, gateway cache, migrations README, API hataları, `web/nul`) | **✅ DONE** (0.1–0.7) |
| 1 | AI katmanı veritabanı migration’ları (`0042`–`0043`) + `system_config` **0044** + locale/tick **0045** | **✅ DONE** |
| 2 | `qtss-ai` crate iskeleti (çoklu `AiCompletionProvider`, on-prem dahil) | **✅ DONE** |
| 3 | Context builder | **✅ DONE** |
| 4 | Taktik AI katmanı | **✅ DONE** |
| 5 | Worker entegrasyonu | **✅ DONE** |
| 6 | Operasyonel / stratejik katman + feedback | **✅ DONE** |
| 7 | API + web UI | **✅ DONE** |
| 8 | `.env.example` AI değişkenleri | **✅ DONE** |
| 9 | Çok dil (web, API, bildirim, AI metinleri, test) | **✅ çekirdek** (9.3 `/me`+locale ✅; 9.2 `error_key` + **`apiErrorFormat`** + **`apiErrors.*`** + **`/locales`** ✅; 9.5 AI locale ✅; `App.tsx` üst şerit + ACP/kanal + **`app.engineDrawer`** + **`app.signalDashboard`** + **`app.paperDrawer`** + komisyon/backfill + çekmece arama ✅; Elliott/bağlam uzun paragraflar isteğe bağlı ince ayar) |
| 10 | **Telegram** + diğer kanallar; sinyal, setup, alım/satım bildirim env ve worker hatları | **✅ DONE** (altyapı + dokümantasyon) |
| 11 | **`system_config` + `app_config`** merkezi yapılandırma | **✅ çekirdek** (11.7 tick taşıması: notify + kill switch + engine + on-chain + position manager + Nansen ✅; 11.10 ✅ lib testleri + CI migrasyon) |

---

## 5. Migration Kuralları

- SQLx sürümü = dosya adındaki sayı öneki (ör. `0042_xxx.sql` → version 42).
- **Aynı önek iki kez kullanılamaz** — SQLx çöker.
- **Squashed checkout:** `migrations/README.md` — yalnız `0001_qtss_baseline.sql`; yeni delta geçici `0002_*.sql` + squash. Üretimde checksum / eski çoklu zincir: **`docs/QTSS_CURSOR_DEV_GUIDE.md` §6**.
- **Tam ürün hattında** (çok dosyalı eski düzen) son migration örneği: **0047** (`worker.kill_switch_*_tick_secs` tohumları); sıradaki boş önek: **0048**. Bu checkout’ta daha kısa bir önek zinciri olabilir — kaynak: `migrations/README.md`.
- Uygulanmış migration dosyasını **asla değiştirme** — checksum uyuşmazlığı. Yeni numara ile yeni dosya ekle.
- Checksum sorunu olursa: `cargo run -p qtss-storage --bin qtss-sync-sqlx-checksums`.
- Her yeni migration sonrası `migrations/README.md` envanterini güncelle.
- Uygulamayı çalıştırmadan önce bekleyen migrasyonların uygulanması: API veya worker başlatıldığında (ör. `cargo run -p qtss-api`) SQLx migrasyonları işlenir — ayrıntı `qtss-storage` / `pool.rs`.
- **Drift uyarısı:** `migrations/README.md` ile `ls migrations/*.sql | sort` aynı sayıda dosya göstermelidir. Uzun hatta **0001–0047**+ beklenir.
- **Çakışan sıra:** Yeni tablolar **0048+**; aynı PR’da çift `NNNN` kullanılmamalıdır.

---

## 6. Ortam Değişkenleri

Kesin kaynak: kök `.env.example`.

### 6.0 AI katmanı (`qtss-ai`) — ortam değişkenleri

Uygulama **`qtss-ai`** içinde çalışır; **kesin örnekler** kök **`.env.example`** içindeki `# === AI (...)` bloğundadır. **Çoklu AI + on-prem:** `app_config.key = ai_engine_config` ile `provider_*` / `model_*` seçilir; env (`QTSS_AI_*`, `ANTHROPIC_*`, `QTSS_AI_OPENAI_COMPAT_*`, `QTSS_AI_OLLAMA_*`) ile üzerine yazılabilir (`config.rs::merge_env_overrides`). Worker tüm AI spawn’larını `QTSS_AI_ENGINE_WORKER=0` ile kapatabilir.

| Değişken | Varsayılan | Açıklama |
|----------|-----------|----------|
| `ANTHROPIC_API_KEY` | (Anthropic kullanılıyorsa zorunlu) | Anthropic tamamlama için. Yapılandırmada bu sağlayıcı seçili değilse boş bırakılabilir. |
| `ANTHROPIC_BASE_URL` | `https://api.anthropic.com` | Anthropic Messages API tabanı; kurumsal proxy veya endpoint yönlendirmesi için değiştirilir (uyumlu gateway şart). |
| `QTSS_AI_MODEL_TACTICAL` | `claude-haiku-4-5-20251001` | Taktik katman modeli |
| `QTSS_AI_MODEL_OPERATIONAL` | `claude-haiku-4-5-20251001` | Operasyonel katman modeli |
| `QTSS_AI_MODEL_STRATEGIC` | `claude-sonnet-4-20250514` | Stratejik katman modeli |
| `QTSS_AI_TACTICAL_TICK_SECS` | 900 | Taktik AI çalışma aralığı (15dk) |
| `QTSS_AI_OPERATIONAL_TICK_SECS` | 120 | Operasyonel AI çalışma aralığı (2dk) |
| `QTSS_AI_STRATEGIC_TICK_SECS` | 86400 | Stratejik AI çalışma aralığı (günde 1) |
| `QTSS_AI_AUTO_APPROVE_ENABLED` | 0 | 1 = otomatik onay aktif |
| `QTSS_AI_AUTO_APPROVE_THRESHOLD` | 0.85 | Otomatik onay için min güven skoru |
| `QTSS_AI_MIN_CONFIDENCE` | 0.60 | Bu altındaki kararlar uygulanmaz |
| `QTSS_AI_STRATEGIC_ENABLED` | 0 | 1 = stratejik katman aktif |
| `QTSS_AI_MAX_POSITION_SIZE_MULT` | 1.5 | AI'ın verebileceği max çarpan |
| `QTSS_AI_DECISION_TTL_SECS` | 1800 | Kararın geçerlilik süresi |

Tablodaki **model kimlikleri** (`claude-haiku-…`, `claude-sonnet-…`) planlama amaçlıdır; gerçek implementasyonda seçilen sağlayıcının resmi model listesi ile doğrulanıp `app_config` / env ile hizalanmalıdır. **On-prem** modeller sunucuda kayıtlı adlarıyla (vLLM/Ollama/TGI) kullanılır; taban URL kurum içi olmalıdır. Ek sağlayıcılar için taban URL, model adı ve kimlik doğrulama kuralları **ilgili `providers/*.rs`** ve runbook ile sabitlenir. Anthropic HTTP sözleşmesi: [Anthropic API — Messages](https://docs.anthropic.com/en/api/messages).

### 6.1 Bildirimler — Telegram ve diğer kanallar

**Servis envanteri:** `qtss-notify` üzerinden **Telegram**, **webhook**, **Discord** (ve crate’te tanımlı diğerleri). Telegram’ı üretimde açmak için tam iş listesi **FAZ 10**’dadır.

**Özet (`.env.example` ile hizalı):** `QTSS_NOTIFY_TELEGRAM_BOT_TOKEN`, `QTSS_NOTIFY_TELEGRAM_CHAT_ID`; genel kuyruk `QTSS_NOTIFY_OUTBOX_*`; **sinyal** (sweep ve/veya ayrı sinyal hattı) `QTSS_NOTIFY_ON_SWEEP_*`, planlı `QTSS_NOTIFY_SIGNAL_*`; **setup** `QTSS_NOTIFY_SETUP_*`; **alım/satım (paper + live)** `QTSS_NOTIFY_PAPER_POSITION_*`, `QTSS_NOTIFY_LIVE_POSITION_*` ve tick env’leri. Kanal seçimi: ilgili `*_CHANNELS` değerine `telegram` yazılır.

### 6.2 `system_config` + `app_config` (FAZ 11)

Uzun vadede çoğu `QTSS_*` ayarı veritabanına taşınır; `.env` öncelikle **`DATABASE_URL`** ve **11.9**’da listelenen bootstrap sırları için tutulur. `app_config` ürün/analiz JSON’ları; **`system_config`** modül bazlı operasyon ayarları (**`0044`**, admin API). Modül listesi ve PR checklist: **`docs/CONFIG_REGISTRY.md`**. Env felaket kurtarma: `QTSS_CONFIG_ENV_OVERRIDES` + `qtss_common::env_override`.

---

## 7. Test Stratejisi

### 7.1 Amaç ve katmanlar

| Katman | Amaç | Bağımlılık | CI |
|--------|------|------------|-----|
| **Birim** | Saf mantık, parser, küçük yardımcılar; hızlı geri bildirim | Yok (veya `sqlx` lib-only) | `rust` job |
| **Entegrasyon (Postgres)** | Migrasyon zinciri, `system_config` tohumları, AI tabloları + SQL sorguları | Çalışan PostgreSQL + `DATABASE_URL` | `postgres-migrations` job |
| **Web** | i18n anahtar bütünlüğü + üretim derlemesi | Node 20 | `web` job |
| **Uçtan uca (isteğe bağlı)** | Worker tick + gerçek WS / borsa; manuel veya ayrı ortam | Tam stack; **§1.3 L1** — şart değil | CI dışı |

**İlke:** Regresyonu erken yakalamak için birim + migrasyon + kritik AI SQL yolları CI’da zorunlu; ağır veya dış ağa bağımlı senaryolar yerelde veya staging’de.

### 7.2 Sürekli entegrasyon (`.github/workflows/ci.yml`)

**`rust` (DB’siz)**

- `cargo test -p qtss-storage --lib`
- `cargo test -p qtss-notify --lib`
- `cargo test -p qtss-common --lib`
- `cargo test -p qtss-worker --bin qtss-worker` (`signal_scorer`, `confluence`, `position_manager`, `strategy_runner`, …)
- `cargo check -p qtss-api -p qtss-worker`

**`postgres-migrations`** — servis `postgres:16-alpine`, `DATABASE_URL=postgres://qtss:qtss@localhost:5432/qtss`

1. `cargo test -p qtss-storage --test migrations_apply` — tüm `migrations/*.sql` uygulanır; AI/`system_config` tablolarının varlığı doğrulanır.
2. `cargo test -p qtss-ai --test decision_exists_for_hash_it` — `decision_exists_for_hash` TTL.
3. `cargo test -p qtss-ai --test maybe_auto_approve_it` — otomatik onay / `pending_approval` koruması.

**`web`**

- `npm ci` → `npm run i18n:check` → `npm run build`

### 7.3 Yerel çalıştırma

```bash
# Tüm workspace (bazı testler DB ister; eksikse Postgres testleri atlanabilir veya hata verir)
cargo test --workspace

# CI ile aynı DB’siz paket
cargo test -p qtss-storage --lib
cargo test -p qtss-notify --lib
cargo test -p qtss-common --lib
cargo test -p qtss-worker --bin qtss-worker

# Postgres entegrasyonları (kök .env veya export)
export DATABASE_URL='postgres://...'
cargo test -p qtss-storage --test migrations_apply -- --nocapture
cargo test -p qtss-ai --test decision_exists_for_hash_it --test maybe_auto_approve_it -- --nocapture
```

**`.env`:** `cargo test` süreci kök `.env` dosyasını otomatik okumaz; entegrasyon testlerinde repo kökünden çalıştırırken ilgili test dosyaları `qtss_common::load_dotenv()` ile kök `.env` içindeki `DATABASE_URL`’ü yükleyebilir. Aksi halde `export DATABASE_URL=...` kullanılır.

### 7.4 Crate / modül özeti

| Alan | Ne test edilir | Not |
|------|----------------|-----|
| `qtss-ai` | `parser`, `safety`, `approval` (birim), `context_builder` (`bar_ohlc_window_metrics`), Postgres: `decision_exists_for_hash`, `maybe_auto_approve` | Tam `build_*_context` async yolu için ortak Postgres fikstürü **isteğe bağlı** (**§1.3 L1**) |
| `qtss-worker` | `bin` target altı birimler; `position_manager::aggregate_long_books` vb. | Canlı borsa HTTP çağrısı **yok** |
| `qtss-storage` | `config_tick`, `system_config` yardımcıları (lib); `migrations_apply` (integration) | |
| `qtss-common` | `config_resolve`, `kill_switch` | |
| `qtss-execution` | `gateway`, `unsupported_live`, `bybit_live` / `okx_live` (orderId / instId parse), `BinanceLiveGateway` birimleri | Ağ üstü place testi yok |
| `web` | i18n senkronu + `vite build` | bileşen snapshot’ı CI’da zorunlu değil |

### 7.5 Kalite kapıları (yerel / PR)

CI workflow’da zorunlu olmayan ama PR öncesi önerilen komutlar:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
```

Projede `-D warnings` ile clippy katılaştırması tanımlıysa aynen kullanılır.

### 7.6 Yeni özellik için kontrol listesi

1. Davranış değişikliği için **birim testi** veya mevcut testin güncellenmesi.
2. SQL/migration değişikliğinde **`migrations_apply`** yerelde ve CI’da yeşil.
3. `qtss-ai` storage/onay mantığı dokunulduysa ilgili **`*_it.rs`** veya yeni entegrasyon testi.
4. Kullanıcıya dönük yeni metinlerde **web** `locales` + `i18n:check` (**FAZ 9**).
5. Harici API veya deterministik olmayan akışlarda **mock** veya yalnızca parse/doğrulama katmanını test et; ağa bağlı flaky test eklemeyin.

### 7.7 Bilinen boşluklar (kasıtlı / sonraki iş)

- **`position_manager`** tam tick + AI + doldurulmuş DB senaryosu (**§1.3 L1**).
- **`build_tactical_context` / `build_operational_context` / `build_strategic_context`** tam async yol için Postgres fikstürü (ağır).
- **Canlı borsa** ve **WebSocket** uçtan uca otomasyon — CI’da yok; manuel/runbook.

---

## 8. Kod Kalitesi Kuralları

1. **Türkçe yorum, İngilizce identifier.** Değişken/fonksiyon/struct/kolon adları İngilizce `snake_case`. Repoda kalıcı kural: `.cursor/rules/english-identifiers.mdc`.
2. **Her loop env'den kontrol edilebilir.** `QTSS_X_ENABLED=0` ile kapatılabilmeli.
3. **Panic yok (üretim döngüleri).** Uzun ömürlü görevlerde geçici hata sonrası `continue` + uygun seviyede log (**§8.1**); kalıcı kusur için `error!` / `log_critical` tercihi.
4. **DB yazımı her zaman upsert.** `INSERT ... ON CONFLICT DO UPDATE`.
5. **Migration dosyası değiştirme.** Yeni numara ile yeni dosya.
6. **`#[must_use]`** skor fonksiyonlarında.
7. **AI kararları deterministic doğrulamadan geçmeli.** `safety.rs` zorunlu — LLM çıktısı doğrudan emir üretemez. Yeni bir **AI sağlayıcı modülü** eklerken aynı `AiRequest`/`AiResponse` ve `parser` + `safety` yolundan geçiş zorunludur; sağlayıcıya özel ham metin sadece `complete` içinde kalır.
8. **Çok dil:** Kullanıcıya görünen yeni metinler (web, API gövdesi, bildirim) mümkün olduğunda çeviri anahtarı + katalog üzerinden eklenmelidir (**FAZ 9**). Worker/API iç log ve hata ayıklama mesajları İngilizce kalabilir (`.cursor/rules/english-identifiers.mdc` ile uyumlu).
9. **Log seviyeleri:** Tüm Rust servisleri (`qtss-api`, `qtss-worker`, ortak crate’ler) ve mümkünse web konsolu **§8.1** tablosuna göre sınıflandırır; yeni kod bu politikaya uyar, mevcut kod modül dokunuşlarında kademeli hizalanır.

### 8.1 Loglama — `trace` / `debug` / `info` / `warn` / `error` / `critical`

**Altyapı:** `tracing` + `tracing-subscriber`. Giriş noktası: `qtss_common::init_logging` (API ve worker `main`). İşlevsel, modül etiketli olaylar için `qtss_common::log_business(QtssLogLevel, module, msg)` veya doğrudan `tracing::*!`. **Kritik** (operatör müdahalesi, uyarıcı hat hatları) için `qtss_common::log_critical(module, msg)` — uygulamada `tracing::error!` ile **`is_critical = true`** alanı (JSON log ve SIEM filtreleri için).

| Seviye | API | Kullanım |
|--------|-----|----------|
| **trace** | `trace!` | En ayrıntılı akış; varsayılan üretim filtrelerinde çoğunlukla kapalı. |
| **debug** | `debug!` | Geliştirici teşhisi: tick içi ayrıntı, sembol listesi, önbellek, isteğe bağlı veri yokluğu (gürültü istemiyorsanız `trace!`). |
| **info** | `info!` | Normal işletme: süreç ayakta, yapılandırma özeti, başarılı tur / snapshot özeti, kullanıcıya güven veren durum. |
| **warning** | `warn!` | Beklenmeyen ama **toparlanabilir**: harici API geçici hatası, retry, eksik opsiyonel config, rate limit yaklaşımı. Arka plan döngülerinde sık tekrar eden uyarılar için mesajı sakin tutun veya `debug!` ile seyreltin. |
| **error** | `error!` | Toparlanması şüpheli veya kalıcı: migrasyon/DB bağlantı kopması, bozuk invariant, istek işlenemiyor (kullanıcıya dönük hata ayrıca `ApiError` ile). |
| **critical** | `log_critical` / `log_business(..., Critical)` | İnsan veya runbook gerekir: ödeme/kredi tükendi, üretim güvenliği (`halt`), veri kaybı riski. |

**Filtreleme:** `RUST_LOG` (örn. `info`, `qtss_worker=debug`, `sqlx::postgres::notice=warn`). **Yapılandırılmış log:** `QTSS_LOG_JSON=1`. **Modül etiketi:** `log_business` / `log_critical` `target: "qtss"` ve `qtss_module` ile gönderir; doğrudan `info!` kullanırken `target`/`module_path` için crate adını anlamlı tutun.

**Web (`web/`):** Aynı anlamsal ayrım `console.debug` / `console.info` / `console.warn` / `console.error` ile; üretimde gereksiz `debug` spam’i kapalı tutulmalı (Vite `import.meta.env.DEV` ile koşullu).

**Bakım:** Büyük refaktörde tek tek dosya taraması yerine, ilgili PR’da değişen dosyalarda seviye gözden geçirmesi yeterlidir; tam repo taraması teknik borç olarak aralıklı yapılabilir.

---

## 9. Güvenlik

Operatör ve OAuth / canlı emir sınırları için ayrıca **`docs/SECURITY.md`**.

- **Bulut** üretici API anahtarları (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY` vb.) — `.env` / secret store; git'e verilmez. **On-prem:** iç ağdaki uç için genelde dış anahtar gerekmez; varsa dahili token/mTLS özelleri yine secret store’da tutulur ve repoya yazılmaz. Kullanılmayan sağlayıcılar için ilgili sırlar tanımlanmayabilir.
- **`QTSS_NOTIFY_TELEGRAM_BOT_TOKEN`** (ve benzeri Telegram sırları) — bot token asla repoya konmaz; üretimde secret store. İlgili sohbet/kanal ID’leri hassas sayılabilir; operatör dokümantasyonunda paylaşım sınırı belirtilir (**FAZ 10.6**).
- AI kararları `is_trading_halted()` kontrolünden geçer.
- `QTSS_AI_MAX_POSITION_SIZE_MULT` — AI'ın verebileceği max çarpan sınırı.
- Auto-approve varsayılan KAPALI (`QTSS_AI_AUTO_APPROVE_ENABLED=0`).
- Her AI kararında `prompt_hash` — aynı bağlama tekrar LLM çağırmaz (maliyet + tutarlılık).
- `ai_decisions.meta_json` — token sayısı, model, **provider_id**, sürüm; audit trail (çoklu sağlayıcı karşılaştırması için).
- **Merkezi config (**FAZ 11**):** `system_config` üzerinden dönen değerler admin API’de rol ile sınırlıdır; `is_secret` satırları listelerde maskelenir. JWT / borsa / LLM anahtarları **aşama 1**’de env veya secret store’da kalır (DB’ye düz metin yazılmaz). Detay: **FAZ 11.9**.

---

## 10. Spawn Sırası

Worker `main.rs` içinde `DATABASE_URL` dolu iken, havuz oluşturulup `run_migrations` sonrası **mevcut** spawn sırası (satır numaraları değişebilir; kanıt: `crates/qtss-worker/src/main.rs`):

```
apply_initial_halt_from_db
tokio::spawn(kill_switch_db_sync_loop)
tokio::spawn(pnl_rollup_loop)
tokio::spawn(binance_catalog_sync_loop)
tokio::spawn(binance_spot_reconcile_loop)
tokio::spawn(binance_futures_reconcile_loop)
tokio::spawn(engine_analysis_loop + WorkerConfluenceHook)
tokio::spawn(engine_symbol_ingest_loop)
tokio::spawn(range_signal_execute_loop)
tokio::spawn(nansen_token_screener_loop)
tokio::spawn(nansen_netflows_loop)
tokio::spawn(nansen_holdings_loop)
tokio::spawn(nansen_perp_trades_loop)
tokio::spawn(nansen_who_bought_loop)
tokio::spawn(nansen_flow_intel_loop)
tokio::spawn(nansen_perp_leaderboard_loop)
tokio::spawn(nansen_whale_perp_aggregate_loop)
tokio::spawn(nansen_setup_scan_loop)
// ensure_binance_sources_for_active_symbols (await, spawn değil)
tokio::spawn(defillama_loop)
tokio::spawn(external_binance_loop)
tokio::spawn(external_coinglass_loop)
tokio::spawn(external_hyperliquid_loop)
tokio::spawn(external_misc_loop)
tokio::spawn(onchain_signal_loop)
tokio::spawn(paper_fill_notify_loop)
tokio::spawn(notify_outbox_loop)
tokio::spawn(live_position_notify_loop)
tokio::spawn(kill_switch_loop)
tokio::spawn(position_manager_loop)
tokio::spawn(copy_trade_follower_loop)
tokio::spawn(copy_trade_queue_loop)
strategy_runner::spawn_if_enabled(&pool).await
ai_engine::spawn_ai_background_tasks(&pool).await   // içinde katman + expire_stale vb.
tokio::spawn(ai_tactical_executor_loop)
binance_user_stream::spawn_binance_user_stream_tasks(&pool).await
```

**Sonra** (aynı `main`, DB’den veya env’den): `engine_symbols` veya `QTSS_KLINE_SYMBOL(S)` ile Binance kline WebSocket görevleri; en sonda isteğe bağlı `QTSS_WORKER_HTTP_BIND` probe HTTP sunucusu.

*Not:* AI ayrıntısı `ai_engine.rs` içinde; yukarıdaki liste özet şemadır — her PR’da `main.rs` ile doğrulayın.

---

## 11. Doküman dizini (`docs/` — 11 Markdown dosyası)

`docs/` altında şu an **11** `.md` dosyası vardır (indeks dahil). Cursor ve geliştirme için **birincil kaynak** `QTSS_MASTER_DEV_GUIDE.md` dosyasıdır. **Not:** `.cursor/rules/` (ör. `english-identifiers.mdc`) bu listede sayılmaz; IDE/agent kuralları ayrıdır.

| Dosya | Rol |
|-------|-----|
| `README.md` | Dizin indeksi |
| `PROJECT.md` | Mimari, crate’ler, API, yol haritası özeti |
| `QTSS_MASTER_DEV_GUIDE.md` | Durum, riskler, iyileştirmeler, AI planı, **FAZ 0–11** (çok dil + Telegram + DB merkezi config) |
| `QTSS_CURSOR_DEV_GUIDE.md` | SQLx / checksum / squash ↔ DB (**§6**); Cursor odaklı |
| `CONFIG_REGISTRY.md` | `system_config` / `app_config` konvansiyonları, admin API özeti (**FAZ 11.3**) |
| `SECURITY.md` | Güvenlik notları |
| `ELLIOTT_V2_STANDARDS.md` | Elliott V2 web referansı |
| `ELLIOTT_WAVE_RULES_REFERENCE.md` | Elliott kuralları referans metni |
| `SPEC_EXECUTION_RANGE_SIGNALS_UI.md` | Execution / range / UI şartnamesi |
| `SIGNAL_MACHINE_GROUP_POLICY.md` | Sinyal makinesi / grup politikası |
| `ACP_PINE_PARITY_FIX.md` | ACP / Pine parity notları |

**Repodan kaldırılan (içerik bu dosyada veya `PROJECT.md` / `.env.example` / kaynak kodda):** `DATA_SOURCES_AND_SOURCE_KEYS.md`, `NANSEN_TOKEN_SCREENER.md`, `SPEC_ONCHAIN_SIGNALS.md`, `PLAN_CONFLUENCE_AND_MARKET_DATA.md`.

**Not:** `PROJECT.md` içindeki yol haritası ve dış linkler bu rehberle uyumlu olmalıdır. Tam anlatım Master’da; SQLx başlatma / checksum / squash konuları için **`QTSS_CURSOR_DEV_GUIDE.md` §6** kullanılır.

---

**Bakım:** Migration son numarası, `migrations/README.md` envanteri, FAZ satırlarındaki **✅ / ❌** durumları, (varsa) kanıt satır sayıları ve güvenlik notlarının güncelliği (`SECURITY.md`) kodla birlikte commit’lerde gözden geçirilmelidir. Büyük özellik PR’larından sonra **Bölüm 0.1** kanıt satırları ve **FAZ 0–11 özet** tablosu gözden geçirilmelidir. Bu dosya projenin birincil geliştirme referansıdır.
