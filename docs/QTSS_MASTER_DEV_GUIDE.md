# QTSS — Master Geliştirme Rehberi (Cursor için)

> **Tarih:** 2026-03-29  
> **Amaç:** Projenin tam durum analizi, tespit edilen hatalar/sorunlar, iyileştirme önerileri ve AI engine entegrasyon planını **tek çatı** altında birleştirir. Bu doküman Cursor'ın ana referansıdır.  
> **Önceki dokümanlar:** Eski ayrı rehberler (`QTSS_CURSOR_DEV_GUIDE.md`, `SPEC_ONCHAIN_SIGNALS.md`, `PLAN_CONFLUENCE_AND_MARKET_DATA.md`, `DATA_SOURCES_AND_SOURCE_KEYS.md`, `NANSEN_TOKEN_SCREENER.md`) ve proje dışı `QTSS_AI_ENGINE_GUIDE` içeriği bu dosyada birleştirildi; o dosyalar repo’dan kaldırıldı. Güncel `docs/` envanteri için **Bölüm 11**’e bakın.

---

## 0. Durum Özeti — Yönlendirmelerim Yapıldı mı?

**Kısa cevap:** **Evet — 16/17 madde tamam.** Tek büyük eksik **AI Engine** (`qtss-ai` crate, `ai_decisions` ailesi tablolar, LLM istemcisi ve katman döngüleri). Mevcut `ai_approval_requests` yalnızca basit bir onay kuyruğu olup gerçek AI engine karar zincirine bağlı değil.

**FAZ 0–8 tabloları** aşağıdaki bölümde yapılacak işleri listeler. Bu tablolardaki maddelerin tamamı şu an **❌** (yapılmadı). Tamamlanan yönlendirmeler bu tabloda değil; **0.1–0.2** numaralı listelerde **✅ DONE** / **❌** ile işaretlidir.

### 0.1 Tamamlanan Maddeler

| # | Yönlendirme | Durum | Dosya / Kanıt |
|---|-------------|-------|---------------|
| 1 | `signal_scorer.rs` — Nansen bileşenleri ayrı skor fonksiyonlarına | ✅ DONE | `score_nansen_netflows`, `_perp_direction`, `_flow_intelligence`, `_buyer_quality`, `_dex_buy_sell_pressure` (514 satır) |
| 2 | `onchain_signal_scorer.rs` — Coinglass/flow-intel çakışma yarı ağırlık | ✅ DONE | `coinglass_netflow_effective` mantığı + `meta_json` izleme (790 satır) |
| 3 | `data_sources/registry.rs` — kayıt sistemi | ✅ DONE | `REGISTERED_DATA_SOURCES` (9) + `REGISTERED_NANSEN_HTTP_KEYS` (8) |
| 4 | `nansen_extended.rs` — tüm HTTP loop'lar | ✅ DONE | 7 loop: netflows, holdings, perp_trades, who_bought, flow_intel, perp_leaderboard, whale_perp_aggregate |
| 5 | `qtss-strategy` crate — 4 strateji + risk | ✅ DONE | signal_filter, whale_momentum, arb_funding, copy_trade, risk, conflict_policy (1098 satır) |
| 6 | `strategy_runner.rs` — DryRunGateway spawn | ✅ DONE | `spawn_if_enabled` + env kontrolü (61 satır) |
| 7 | `position_manager.rs` — SL/TP + dry/live close | ✅ DONE | Dry ve live yol ayrımı, `is_trading_halted()` kontrolü (383 satır) |
| 8 | `kill_switch.rs` — drawdown koruması | ✅ DONE | `halt_trading` + `QTSS_MAX_DRAWDOWN_PCT` (91 satır) |
| 9 | `confluence.rs` — rejim ağırlıklı bileşik skor | ✅ DONE | `default_weights_by_regime`, `lot_scale_hint`, `direction_from_composite_score` (558 satır) |
| 10 | Çoklu sembol WS | ✅ DONE | `multi_kline_ws_loop` + combined URL |
| 11 | Copy trade kuyruğu | ✅ DONE | Migration 0037 + `copy_trade_queue.rs` + `copy_trade_follower.rs` |
| 12 | AI onay kuyruğu (basit) | ✅ DONE | Migration 0038 + API routes (list/create/decide) |
| 13 | Notify outbox | ✅ DONE | Migration 0039 + worker loop + API |
| 14 | User permissions + audit | ✅ DONE | Migration 0040-0041 + RBAC + admin CRUD |
| 15 | CI pipeline | ✅ DONE | `rust-ci.yml` — cargo check/test/audit + web build |
| 16 | Probe endpoints | ✅ DONE | `/live`, `/ready`, `/metrics`, worker HTTP bind |

### 0.2 YAPILMAYAN Tek Büyük Madde

| # | Yönlendirme | Durum | Açıklama |
|---|-------------|-------|----------|
| 17 | **AI Engine (`qtss-ai` crate + LLM entegrasyonu)** | ❌ YAPILMADI | `qtss-ai` crate yok. `ai_decisions`, `ai_tactical_decisions`, `ai_position_directives`, `ai_decision_outcomes` tabloları yok. LLM istemcisi, context_builder, parser, katmanlı karar döngüleri — hiçbiri yok. Mevcut `ai_approval_requests` sadece basit bir onay kuyruğu olup AI engine'in karar zincirinden bağımsız. |

---

## 1. Tespit Edilen Hatalar ve Sorunlar

### 1.1 KRİTİK — Çalışma Zamanı Riskleri

**H1: `position_manager.rs` — gateway her tick'te yeniden oluşturuluyor olabilir**
- Sorun: `position_manager_loop(pool: PgPool)` imzasında gateway yok; canlı yolda her tick'te `BinanceLiveGateway` oluşturulma maliyeti var.
- Çözüm: Gateway'i loop başında bir kez oluştur, `Arc` ile taşı. Env değişikliği restart gerektirir zaten.

**H2: `kill_switch.rs` — halt sonrası geri alma mekanizması yok**
- Sorun: `halt_trading()` global `AtomicBool` set ediyor; geri dönüş yolu sadece worker restart.
- Etki: Gece tetiklenen kill switch sabaha kadar tüm strateji/pozisyon yönetimini devre dışı bırakır.
- Çözüm: API'den `POST /api/v1/admin/kill-switch/reset` endpoint'i + `resume_trading()` fonksiyonu ekle. Admin rolü zorunlu. Audit log kaydı.

**H3: `confluence.rs` — eksik veri ile nötr sinyalin ayrılmaması**
- Sorun: `fetch_data_snapshot` `None` dönerse bileşen 0.0 olarak hesaba katılıyor. Veri yokluğu "nötr sinyal" ile eşleştirilmiş.
- Etki: 3 kaynaktan 2'si down olunca, tek kalan kaynağın skoru bileşik skoru tek başına belirler; `confidence` bunu yansıtmaz.
- Çözüm: Bileşen katkısında `data_available_count / total_expected_count` ile `confidence` düşürmeli. `meta_json.components_missing` flag'i ekle.

**H4: `strategy_runner.rs` — 4 strateji aynı sanal bakiyeyi paylaşıyor**
- Sorun: `dry_gateway_from_env()` tek gateway → 4 strateji aynı `VirtualLedgerParams.initial_quote_balance`.
- Etki: Bir strateji tüm bakiyeyi tüketirse diğerleri `InsufficientPaper` alır.
- Çözüm: Her strateji kendi gateway'ini oluştursun. Bakiye: `QTSS_STRATEGY_RUNNER_QUOTE_BALANCE_USDT / 4` veya strateji bazlı env.

### 1.2 ORTA — Tasarım Sorunları

**M1: API hata dönüş tipi `Result<..., String>` tutarsız**
- Sorun: `ai_approval.rs`, `reconcile.rs`, `analysis.rs` vb. route handler'lar `Result<Json<T>, String>` dönüyor. Axum bu durumda 500 Internal Server Error ile düz metin gövde döner.
- Etki: İstemci yapılandırılmış hata JSON'ı alamaz; hata kodu (400 vs 404 vs 500) ayrılmaz.
- Çözüm: Ortak `ApiError` enum'u (`AppError(StatusCode, serde_json::Value)`) + `IntoResponse` impl. Mevcut raporlama değerini JSON `{"error": "..."}` olarak sar.

**M2: `main.rs` (worker) — `SinkExt` import edilmiş ama kullanılmıyor**
- Sorun: `use futures_util::{SinkExt, StreamExt};` — `SinkExt` yalnızca `ws.send(Message::Pong(...))` için gerekli; `send` `SinkExt`'ten geliyor.
- Durum: Aslında kullanılıyor ama implicit trait import. Açıklayıcı yorum eklenmeli.

**M3: `web/nul` — Windows artifact dosyası**
- Sorun: Windows’ta `web/` altında yanlış yönlendirme ile `nul` adlı dosya oluşabiliyordu.
- Durum: Dosya repodan silindi; kök `.gitignore` içinde `web/nul` ignore ediliyor.

**M4: Exchange `"binance"` hardcoded — çoklu borsa genişlemesini zorlaştırır**
- Sorun: `main.rs` içinde `let exchange = "binance"` sabit. Kline WS loop'ları yalnız Binance'a bağlı.
- Çözüm: Şimdilik sorun değil ama yeni borsa eklendiğinde env'den veya config'den okunmalı.

**M5: `ai_approval_requests` ile planlanan `ai_decisions` arasında şema çatışması riski**
- Sorun: Mevcut `0038_ai_approval_requests.sql` basit bir onay kuyruğu (`org_id`, `kind`, `payload`). AI engine planındaki `ai_decisions` tablosu tamamen farklı bir yapı (`layer`, `model_id`, `prompt_hash`, `parsed_decision`, `expires_at`, `confidence`).
- Çözüm: İkisi farklı tablolar olarak kalmalı. `ai_approval_requests` genel amaçlı onay; `ai_decisions` LLM karar zinciri. `ai_decisions`'da `approval_request_id` FK ile bağlanabilir.

### 1.3 DÜŞÜK — İyileştirme Fırsatları

**L1: Test coverage düşük** — `signal_scorer.rs`'de birim testler var ama `confluence.rs`, `position_manager.rs`, `kill_switch.rs`, `strategy_runner.rs` için test yok.

**L2: `pnl_rollup_loop` 1 saat tick** — PnL rollup yalnızca saatlik çalışıyor. Kill switch günlük P&L toplamını okuyor. Kill switch 60s tick ama P&L verisi 1 saat gecikebilir.

**L3: Migrations README envanter sayımı 0001-0036 diyor ama gerçekte 0001-0041 var** — README güncellenmemiş.

**L4: `docs/ELLIOTT_V2_STANDARDS.md` projede aktif kullanılmıyor** — Elliott V2 engine `web/src/lib/elliottEngineV2/` altında JS/TS; bu doküman referans ama güncelliğinden emin olunmalı.

---

## 2. İyileştirme Önerileri

### 2.1 Kısa Vadeli (Hemen)

1. **Kill switch reset endpoint** — `POST /api/v1/admin/kill-switch/reset` + `qtss_common::resume_trading()`. Admin rolü zorunlu. Tetiklendiğinde audit log kaydı.

2. **API error standardizasyonu** — `crates/qtss-api/src/error.rs` → `ApiError` enum. Tüm route handler'lar `Result<Json<T>, ApiError>` dönsün. HTTP 4xx/5xx ayrımı yapılsın.

3. **Strateji başına ayrı DryRunGateway** — `strategy_runner.rs`'de her strateji kendi bakiye bütçesini alsın.

4. **Migrations README güncelle** — 0037-0041 ekle.

5. **`web/nul`** — repodan silindi; `.gitignore`’da `web/nul` (bak. M3, FAZ 0.6).

### 2.2 Orta Vadeli (AI Engine öncesi)

6. **Confluence confidence skoru** — `data_available_count` / `total_expected_count` ile düşürülen confidence. Bu, AI engine'in bağlamı doğru anlamasını da sağlar.

7. **PnL rollup sıklığı** — Kill switch ile uyumlu olması için en az 5dk'da bir veya kill switch'in kendi mini PnL hesabı.

8. **Position manager gateway caching** — Loop başında bir kez oluştur, `Arc` ile paylaş.

9. **Integration test altyapısı** — CI'da Postgres servisli job ekle. En azından migration + seed + temel sorgu testi.

### 2.3 Uzun Vadeli (AI Engine sonrası)

10. **Trailing stop desteği** — Mevcut `position_manager.rs`'de trailing stop yok. AI engine'in `activate_trailing` direktifi şu an uygulanamaz. `OrderType::TrailingStopMarket` + Binance `TRAILING_STOP_MARKET` emri.

11. **WebSocket fill stream** — Copy trade ve reconcile için Binance user stream entegrasyonu. Daha hızlı dolum algılama.

12. **Çoklu borsa adapter** — `ExecutionGateway` trait'i hazır; `BybitGateway`, `OKXGateway` gibi yeni borsa implementasyonları.

---

## 3. AI Engine Entegrasyon Planı — Güncel Durum

### 3.1 Mimari Felsefe

Mevcut sistem kural tabanlı skor matrisleri ile çalışıyor (`signal_scorer` → `onchain_signal_scorer` → `confluence`). AI bu sistemi **değiştirmez, güçlendirir:**

```
Mevcut:  Veri → Kural tabanlı skor → Emir
Hedef:   Veri → AI analizi (async, periyodik) → AI kararı (JSON) → DB
                                                                    ↓
         Veri → Kural tabanlı skor ────────────────────────────→ Emir (AI bilgisiyle zenginleştirilmiş)
```

AI **danışman** rolündedir: periyodik LLM çağrısı → yapılandırılmış JSON → DB. Yürütme katmanı bu JSON'u okur ama AI çökmüş olsa bile kural tabanlı modda çalışmaya devam eder.

### 3.2 Katman Mimarisi

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

Mevcut `ai_approval_requests` tablosu (migration 0038) genel amaçlı onay kuyruğu olarak kalacak. AI engine'in `ai_decisions` tablosu ayrı — ama onay gerektiren AI kararları `ai_approval_requests`'e de yazılarak operatör onayı alınabilir. Bu iki sistem birbirini tamamlar.

---

## 4. Cursor İçin Sıralı Görev Listesi

Aşağıdaki **FAZ 0–8** maddeleri tek tek **❌** veya tamamlandığında **✅ DONE** ile güncellenmelidir. (Şu anki kod tabanında bu fazların alt görevleri bekliyor; tarihsel olarak biten işler **Bölüm 0** tablolarında **✅ DONE** olarak listelenir.)

### FAZ 0 — Mevcut Hata Düzeltmeleri (AI öncesi temizlik)

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 0.1 | Kill switch reset endpoint | `qtss-api/src/routes/`, `qtss-common/src/kill_switch.rs` | `POST /api/v1/admin/kill-switch/reset` → `resume_trading()` + audit log. Admin rolü zorunlu. `qtss-common`'a `pub fn resume_trading()` ekle (AtomicBool false). API route: body `{"confirm": true}` zorunlu. | ❌ |
| 0.2 | Strategy runner bakiye izolasyonu | `qtss-worker/src/strategy_runner.rs` | `dry_gateway_from_env()` yerine `dry_gateway_for_strategy(name: &str)` → her strateji için `QTSS_STRATEGY_RUNNER_QUOTE_BALANCE_USDT / 4` veya `QTSS_STRATEGY_{NAME}_BALANCE` env. | ❌ |
| 0.3 | Confluence confidence düşürme | `qtss-worker/src/confluence.rs` | `compute_and_persist` içinde: kaç bileşenin verisi geldi say; `data_missing_count > 0` ise `confidence *= (available / total)`. `meta_json.components_missing` listesi ekle. | ❌ |
| 0.4 | Position manager gateway caching | `qtss-worker/src/position_manager.rs` | `position_manager_loop` başında bir kez `Arc<dyn ExecutionGateway>` oluştur (dry veya live); her tick'te `clone()` et. | ❌ |
| 0.5 | Migrations README güncelle | `migrations/README.md` | 0037-0041 migration'ları envantere ekle. Sonraki boş numara: **0042**. | ❌ |
| 0.6 | `web/nul` sil + ignore | `web/nul`, `.gitignore` | Repo’dan kaldırıldı; `.gitignore`’da `web/nul`. | ✅ DONE |
| 0.7 | API error standardizasyonu (opsiyonel, büyük refactor) | `qtss-api/src/` | `ApiError` enum + `IntoResponse`. Tüm `Result<..., String>` dönüşlerini değiştir. **Not:** Bu büyük bir refactor — AI engine route'larını doğrudan `ApiError` ile yazılabilir; eski route'lar kademeli taşınır. | ❌ |

### FAZ 1 — AI Engine Veritabanı Altyapısı

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 1.1 | Migration: `0042_ai_engine_tables.sql` | `migrations/0042_ai_engine_tables.sql` | Aşağıdaki tabloları oluştur: `ai_decisions` (id, created_at, layer CHECK('strategic','tactical','operational'), symbol NULL, model_id TEXT, prompt_hash TEXT, input_snapshot JSONB, raw_output TEXT, parsed_decision JSONB, status CHECK('pending_approval','approved','applied','rejected','expired','error'), approved_by TEXT, approved_at, applied_at, expires_at, confidence FLOAT8, meta_json JSONB). İndeksler: `(symbol, layer, created_at DESC)`, `(status) WHERE status IN ('pending_approval','approved')`. | ❌ |
| 1.2 | Migration: `ai_tactical_decisions` tablosu | 0042 içinde | `ai_tactical_decisions` (id, decision_id FK→ai_decisions, created_at, valid_until, symbol TEXT NOT NULL, direction CHECK('strong_buy','buy','neutral','sell','strong_sell','no_trade'), position_size_multiplier FLOAT8 DEFAULT 1.0, entry_price_hint FLOAT8, stop_loss_pct FLOAT8, take_profit_pct FLOAT8, reasoning TEXT, confidence FLOAT8, status CHECK('pending_approval','approved','applied','rejected','expired')). İndeks: `(symbol, status, created_at DESC)`. | ❌ |
| 1.3 | Migration: `ai_position_directives` tablosu | 0042 içinde | `ai_position_directives` (id, decision_id FK, created_at, symbol NOT NULL, open_position_ref UUID, action CHECK('keep','tighten_stop','widen_stop','activate_trailing','deactivate_trailing','partial_close','full_close','add_to_position'), new_stop_loss_pct, new_take_profit_pct, trailing_callback_pct, partial_close_pct, reasoning, status CHECK(...)). | ❌ |
| 1.4 | Migration: `ai_portfolio_directives` tablosu | 0042 içinde | `ai_portfolio_directives` (id, decision_id FK, created_at, valid_until, risk_budget_pct FLOAT8, max_open_positions INT, preferred_regime TEXT, symbol_scores JSONB, macro_note TEXT, status TEXT DEFAULT 'active'). | ❌ |
| 1.5 | Migration: `ai_decision_outcomes` tablosu | 0042 içinde | `ai_decision_outcomes` (id, decision_id FK, recorded_at, pnl_pct, pnl_usdt, outcome CHECK('profit','loss','breakeven','expired_unused'), holding_hours, notes). | ❌ |
| 1.6 | Migration: `0043_ai_engine_config.sql` | `migrations/0043_ai_engine_config.sql` | `app_config` seed: key `ai_engine_config`, value JSON: `{"enabled": false, "tactical_layer_enabled": true, "operational_layer_enabled": true, "strategic_layer_enabled": false, "auto_approve_threshold": 0.85, "auto_approve_enabled": false, "tactical_tick_secs": 900, "operational_tick_secs": 120, "strategic_tick_secs": 86400, "model_tactical": "claude-haiku-4-5-20251001", "model_operational": "claude-haiku-4-5-20251001", "model_strategic": "claude-sonnet-4-20250514", "max_tokens_tactical": 1024, "max_tokens_operational": 512, "max_tokens_strategic": 4096, "decision_ttl_secs": 1800, "require_min_confidence": 0.60}`. `ON CONFLICT (key) DO NOTHING`. | ❌ |
| 1.7 | `migrations/README.md` güncelle | `migrations/README.md` | 0042 ve 0043 satırlarını ekle. | ❌ |

### FAZ 2 — `qtss-ai` Crate İskeleti

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 2.1 | Crate oluştur + workspace'e ekle | `crates/qtss-ai/Cargo.toml`, kök `Cargo.toml` | `[package] name = "qtss-ai"`. Dependencies: tokio, tracing, serde, serde_json, chrono, uuid, anyhow, thiserror, async-trait, sqlx, reqwest (`features = ["json", "rustls-tls"]`), sha2, hex + workspace üyeleri (qtss-common, qtss-domain, qtss-storage, qtss-notify). Kök Cargo.toml'a `"crates/qtss-ai"` member + `qtss-ai = { path = "crates/qtss-ai" }` ekle. | ❌ |
| 2.2 | `src/lib.rs` — modül tanımları | `crates/qtss-ai/src/lib.rs` | `pub mod client; pub mod context_builder; pub mod parser; pub mod layers; pub mod storage; pub mod approval; pub mod safety;` Re-export: `pub use client::AiClient;` | ❌ |
| 2.3 | `src/client.rs` — Anthropic Messages API istemcisi | `crates/qtss-ai/src/client.rs` | `AiClient::from_env()` → `ANTHROPIC_API_KEY` + `ANTHROPIC_BASE_URL` (varsayılan `https://api.anthropic.com`). `pub async fn complete(&self, req: &AiRequest) -> Result<AiResponse>`. `AiRequest`: model, system_prompt, user_message, max_tokens, temperature. `AiResponse`: content, input_tokens, output_tokens, model. HTTP: `POST {base_url}/v1/messages`, headers: `x-api-key`, `anthropic-version: 2023-06-01`. Timeout: 120s. Hata durumunda ilk 500 karakter log. **`Clone` derive** — worker'da birden çok spawn'a kopyalanacak. | ❌ |
| 2.4 | `src/storage.rs` — AI tablo DB fonksiyonları | `crates/qtss-ai/src/storage.rs` | `insert_ai_decision(pool, layer, symbol, model_id, prompt_hash, input_snapshot, raw_output, parsed_decision, confidence) -> Result<Uuid>`. `insert_tactical_decision(pool, decision_id, symbol, parsed, valid_until) -> Result<Uuid>`. `insert_position_directive(pool, ...)`. `insert_portfolio_directive(pool, ...)`. `fetch_latest_approved_tactical(pool, symbol) -> Option<Row>`. `fetch_latest_approved_directive(pool, symbol) -> Option<Row>`. `mark_applied(pool, table, id)`. `expire_stale_decisions(pool)` — `status='pending_approval' AND expires_at < now()` → `status='expired'`. `decision_exists_for_hash(pool, hash, ttl_minutes) -> bool`. | ❌ |
| 2.5 | `src/parser.rs` — LLM JSON ayrıştırıcı | `crates/qtss-ai/src/parser.rs` | `parse_tactical_decision(raw: &str) -> Result<Value>`: JSON blok çıkarma (```json...``` veya ham {}), `direction` zorunlu (strong_buy/buy/neutral/sell/strong_sell/no_trade), `confidence` zorunlu (0.0-1.0), `position_size_multiplier` sınır (0.0-2.0). `parse_operational_decision(raw) -> Result<Value>`: `action` zorunlu (keep/tighten_stop/widen_stop/activate_trailing/...). `extract_json_block(raw) -> String`: yardımcı. **Birim testleri:** Her parse fonksiyonu için en az 3 test (geçerli, geçersiz direction, eksik alan). | ❌ |
| 2.6 | `src/safety.rs` — güvenlik doğrulama | `crates/qtss-ai/src/safety.rs` | `validate_ai_decision_safety(decision: &Value, config: &SafetyConfig) -> Result<(), &'static str>`: (1) `position_size_multiplier <= config.max_size_multiplier`, (2) `stop_loss_pct` zorunlu (buy/sell kararlarında), (3) `qtss_common::is_trading_halted()` kontrolü. `SafetyConfig`: `max_size_multiplier` (env `QTSS_AI_MAX_POSITION_SIZE_MULT`, varsayılan 1.5). | ❌ |

### FAZ 3 — Context Builder (DB → LLM Bağlamı)

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 3.1 | `src/context_builder.rs` — taktik bağlam | `crates/qtss-ai/src/context_builder.rs` | `pub async fn build_tactical_context(pool, symbol) -> Result<Value>`: (1) `fetch_latest_onchain_signal_score(pool, symbol)` → aggregate_score, confidence, direction, conflict_detected, funding_score, nansen_sm_score, (2) `fetch_analysis_snapshot(pool, symbol, "confluence")` → composite_score, regime, pillar_scores, (3) `market_bars` son 20 mum → son fiyat, 24h değişim %, volatilite (high-low range / close ortalaması), (4) `exchange_orders` açık pozisyon özeti (entry, size, side, unrealized_pnl_pct), (5) Son AI kararı (24h içi, tekrar aynı kararı vermemek için). Çıktı: `{"symbol", "timestamp_utc", "onchain_signals", "confluence", "price_context", "open_position", "last_ai_decision"}`. **Token bütçesi:** ~2000 token; ham bar yerine istatistik özeti. | ❌ |
| 3.2 | `context_builder.rs` — operasyonel bağlam | Aynı dosya | `pub async fn build_operational_context(pool, symbol) -> Result<Value>`: Sadece açık pozisyon varsa çalışır. Açık pozisyon özeti + son 5 mum + funding snapshot + onchain özet (aggregate_score, direction, conflict_detected). ~1000 token. | ❌ |
| 3.3 | `context_builder.rs` — stratejik bağlam | Aynı dosya | `pub async fn build_strategic_context(pool) -> Result<Value>`: Tüm sembollerin son confluence skorları + 7 günlük PnL özeti + portföy maruz kalma. ~8000 token. | ❌ |
| 3.4 | `qtss-storage` — eksik yardımcı fonksiyonlar | `crates/qtss-storage/src/` | Eğer eksikse ekle: `fetch_latest_onchain_signal_score(pool, symbol) -> Option<OnchainSignalScoreRow>`, `fetch_open_positions_summary(pool, symbol)` (exchange_orders dolmuş ama kapanmamış net long), `fetch_recent_bars_stats(pool, symbol, n)` (son n mum istatistiği). Bu fonksiyonlar `context_builder`'ın DB okumasını sağlar. | ❌ |

### FAZ 4 — Taktik AI Katmanı (En Kritik)

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 4.1 | `src/layers/mod.rs` | `crates/qtss-ai/src/layers/mod.rs` | `pub mod tactical; pub mod operational; pub mod strategic;` | ❌ |
| 4.2 | `src/layers/tactical.rs` — taktik karar döngüsü | `crates/qtss-ai/src/layers/tactical.rs` | `TacticalLayer { pool, client }` + `pub async fn run(self)`. Tick: `QTSS_AI_TACTICAL_TICK_SECS` (varsayılan 900). Her tick: (1) `ai_engine_enabled` kontrolü (app_config'den), (2) `list_enabled_engine_symbols`, (3) Her sembol için: `build_tactical_context` → `hash_context` (SHA-256) → `decision_exists_for_hash` (30dk TTL) kontrolü → LLM çağrısı → `parse_tactical_decision` → safety validation → `insert_ai_decision` + `insert_tactical_decision` → `maybe_auto_approve`. Sistem promptu: JSON-only, direction/confidence/stop_loss_pct zorunlu, `no_trade` geçerli, `temperature: 0.3`. Hata durumunda `insert_ai_decision_error` (status='error'). `no_trade` kararı DB'ye yazılmaz, sadece log. Minimum confidence (app_config `require_min_confidence`, varsayılan 0.60) altı → skip. | ❌ |
| 4.3 | `src/approval.rs` — otomatik onay | `crates/qtss-ai/src/approval.rs` | `maybe_auto_approve(pool, decision_id, confidence)`: `QTSS_AI_AUTO_APPROVE_ENABLED=1` VE `confidence >= threshold` → `ai_decisions.status='approved'` + `ai_tactical_decisions.status='approved'`. Değilse: `qtss-notify` ile Telegram/webhook bildirim (sembol, direction, confidence, reasoning). | ❌ |
| 4.4 | `src/layers/tactical.rs` — sistem promptu | Aynı dosya | Türkçe reasoning, JSON-only, karar kriterleri: `aggregate_score > 0.6 AND !conflict → buy/strong_buy`, `< -0.6 AND !conflict → sell/strong_sell`, `conflict → multiplier 0.5 veya no_trade`, `zaten açık pozisyon + aynı yön → no_trade`. `confidence < 0.5 → no_trade`. | ❌ |

### FAZ 5 — Worker Entegrasyonu

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 5.1 | `qtss-worker` Cargo.toml'a `qtss-ai` ekle | `crates/qtss-worker/Cargo.toml` | `qtss-ai.workspace = true` | ❌ |
| 5.2 | `main.rs`'e AI spawn'ları ekle | `crates/qtss-worker/src/main.rs` | DATABASE_URL bloğu sonunda: `if let Ok(ai_client) = qtss_ai::AiClient::from_env() { tokio::spawn(tactical_layer.run()); tokio::spawn(operational::run(..)); if strategic_enabled { tokio::spawn(strategic::run(..)); } } else { warn!("ANTHROPIC_API_KEY tanımsız — AI engine kapalı"); }`. Ana döngüde ek: `tokio::spawn(qtss_ai::storage::expire_stale_decisions_loop(pool))` — 5dk tick ile süresi dolmuş kararları temizle. | ❌ |
| 5.3 | `position_manager.rs`'de AI kararlarını oku | `crates/qtss-worker/src/position_manager.rs` | Her tick'te (mevcut SL/TP kontrolünden ÖNCE): (1) `SELECT * FROM ai_tactical_decisions WHERE symbol=$1 AND status='approved' AND valid_until > now() ORDER BY created_at DESC LIMIT 1`. Varsa: `effective_sl = td.stop_loss_pct.unwrap_or(default_sl)`, `effective_tp = td.take_profit_pct.unwrap_or(default_tp)`, `effective_multiplier = td.position_size_multiplier.clamp(0.0, 2.0)`. Uygulandıktan sonra: `UPDATE ai_tactical_decisions SET status='applied'`. (2) `SELECT * FROM ai_position_directives WHERE symbol=$1 AND status='approved' AND created_at > now() - interval '10 min' ORDER BY created_at DESC LIMIT 1`. Varsa: `match action { "tighten_stop" => ..., "activate_trailing" => ..., "partial_close" => ..., "full_close" => ... }`. **AI yoksa:** Mevcut kural tabanlı mantık aynen çalışır — geriye uyumluluk korunur. | ❌ |

### FAZ 6 — Operasyonel ve Stratejik Katmanlar

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 6.1 | `src/layers/operational.rs` | `crates/qtss-ai/src/layers/operational.rs` | 2dk tick. Yalnızca açık pozisyon olan semboller için çalışır. `build_operational_context` → LLM → `parse_operational_decision` → `insert_position_directive` → `maybe_auto_approve`. Sistem promptu: trailing stop kararı, stop güncelleme (kötüleştirilemez), partial/full close. | ❌ |
| 6.2 | `src/layers/strategic.rs` | `crates/qtss-ai/src/layers/strategic.rs` | Günde 1 (86400s). `build_strategic_context` → büyük model (Sonnet) → `insert_portfolio_directive`. Çıktı: risk_budget_pct, max_open_positions, preferred_regime, symbol_scores. Taktik katman bu direktifleri okuyarak sembol ağırlıklarını ayarlar. `QTSS_AI_STRATEGIC_ENABLED=1` ile açılır. | ❌ |
| 6.3 | Öğrenme döngüsü (feedback) | `crates/qtss-ai/src/feedback.rs` | Pozisyon kapandığında `ai_decision_outcomes`'a kayıt. Stratejik katman son 30 kararın win_rate, avg_pnl, best_regime istatistiğini bağlama dahil eder. Gerçek ML training yok — LLM geçmiş performansı bağlamdan okur. | ❌ |

### FAZ 7 — API Endpoints + Web UI

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 7.1 | AI karar API'leri | `crates/qtss-api/src/routes/ai_decisions.rs` | `GET /api/v1/ai/decisions?layer=&symbol=&status=&limit=` — tüm roller okuyabilir. `GET /api/v1/ai/decisions/{id}` — detay. `POST /api/v1/ai/decisions/{id}/approve` — admin. `POST /api/v1/ai/decisions/{id}/reject` — admin. `GET /api/v1/ai/directives/tactical?symbol=` — son onaylı taktik karar. `GET /api/v1/ai/directives/portfolio` — aktif portföy direktifi. | ❌ |
| 7.2 | Web UI: AI kararları paneli | `web/src/components/AiDecisionsPanel.tsx` | Taktik kararlar listesi (sembol, direction, confidence, status, reasoning). Pending kararları onaylama/reddetme butonları (admin). Son portföy direktifi kartı. | ❌ |

### FAZ 8 — Ortam Değişkenleri + .env.example

| # | Görev | Dosya(lar) | Detay | Durum |
|---|-------|-----------|-------|-------|
| 8.1 | `.env.example`'a AI env'leri ekle | `.env.example` | Aşağıdaki blok: `# === AI Engine (qtss-ai) ===`, `ANTHROPIC_API_KEY=`, `# ANTHROPIC_BASE_URL=https://api.anthropic.com`, `# QTSS_AI_MODEL_TACTICAL=claude-haiku-4-5-20251001`, `# QTSS_AI_MODEL_OPERATIONAL=claude-haiku-4-5-20251001`, `# QTSS_AI_MODEL_STRATEGIC=claude-sonnet-4-20250514`, `# QTSS_AI_TACTICAL_TICK_SECS=900`, `# QTSS_AI_OPERATIONAL_TICK_SECS=120`, `# QTSS_AI_STRATEGIC_TICK_SECS=86400`, `# QTSS_AI_AUTO_APPROVE_ENABLED=0`, `# QTSS_AI_AUTO_APPROVE_THRESHOLD=0.85`, `# QTSS_AI_MIN_CONFIDENCE=0.60`, `# QTSS_AI_STRATEGIC_ENABLED=0`, `# QTSS_AI_MAX_POSITION_SIZE_MULT=1.5`, `# QTSS_AI_DECISION_TTL_SECS=1800` | ❌ |

**FAZ 0–8 üst seviye özet**

| FAZ | Kapsam | Durum |
|-----|--------|--------|
| 0 | AI öncesi temizlik (kill switch reset, bakiye izolasyonu, confluence confidence, gateway cache, migrations README, API hataları, `web/nul`) | ❌ (yalnız **0.6** `web/nul` + ignore: **✅ DONE**) |
| 1 | AI engine veritabanı migration’ları | ❌ |
| 2 | `qtss-ai` crate iskeleti | ❌ |
| 3 | Context builder | ❌ |
| 4 | Taktik AI katmanı | ❌ |
| 5 | Worker entegrasyonu | ❌ |
| 6 | Operasyonel / stratejik katman + feedback | ❌ |
| 7 | API + web UI | ❌ |
| 8 | `.env.example` AI değişkenleri | ❌ |

---

## 5. Migration Kuralları

- SQLx sürümü = dosya adındaki sayı öneki (ör. `0042_xxx.sql` → version 42).
- **Aynı önek iki kez kullanılamaz** — SQLx çöker.
- Mevcut son migration: **0041** (`audit_log_details`). Sonraki boş: **0042**.
- Uygulanmış migration dosyasını **asla değiştirme** — checksum uyuşmazlığı. Yeni numara ile yeni dosya ekle.
- Checksum sorunu olursa: `cargo run -p qtss-storage --bin qtss-sync-sqlx-checksums`.
- Her yeni migration sonrası `migrations/README.md` envanterini güncelle.

---

## 6. Ortam Değişkenleri

Kesin kaynak: kök `.env.example`. Bu bölümde yalnızca **yeni AI engine** değişkenleri listelenir:

| Değişken | Varsayılan | Açıklama |
|----------|-----------|----------|
| `ANTHROPIC_API_KEY` | (zorunlu) | Anthropic API anahtarı. Boşsa AI engine başlamaz, sistem kural tabanlı çalışır. |
| `ANTHROPIC_BASE_URL` | `https://api.anthropic.com` | OpenAI uyumlu proxy kullanmak için değiştir. |
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

---

## 7. Test Stratejisi

**Mevcut testler:** `cargo test -p qtss-worker` — `signal_scorer.rs` birim testleri.

**Yeni AI engine testleri (Cursor eklemeli):**

1. `parser.rs` — `parse_tactical_decision` ve `parse_operational_decision` için en az 5 test: geçerli JSON, geçersiz direction, eksik confidence, out-of-range multiplier, `\`\`\`json` wrapped.
2. `safety.rs` — `validate_ai_decision_safety` testi: max multiplier aşımı, SL eksik, kill switch aktif.
3. `context_builder.rs` — mock DB ile: onchain skoru var/yok, açık pozisyon var/yok.
4. `storage.rs` — `decision_exists_for_hash` TTL testi.
5. `approval.rs` — auto-approve threshold testi.

```bash
cargo test -p qtss-ai
cargo test -p qtss-worker
```

---

## 8. Kod Kalitesi Kuralları

1. **Türkçe yorum, İngilizce identifier.** Değişken/fonksiyon/struct/kolon adları İngilizce `snake_case`.
2. **Her loop env'den kontrol edilebilir.** `QTSS_X_ENABLED=0` ile kapatılabilmeli.
3. **Hata: `warn!` yaz, panic etme.** Loop'lar `loop { if err { warn!(); sleep(); continue; } }`.
4. **DB yazımı her zaman upsert.** `INSERT ... ON CONFLICT DO UPDATE`.
5. **Migration dosyası değiştirme.** Yeni numara ile yeni dosya.
6. **`#[must_use]`** skor fonksiyonlarında.
7. **AI kararları deterministic doğrulamadan geçmeli.** `safety.rs` zorunlu — LLM çıktısı doğrudan emir üretemez.

---

## 9. Güvenlik

- `ANTHROPIC_API_KEY` — `.env`'de, git'e verilmez. Üretimde secret store.
- AI kararları `is_trading_halted()` kontrolünden geçer.
- `QTSS_AI_MAX_POSITION_SIZE_MULT` — AI'ın verebileceği max çarpan sınırı.
- Auto-approve varsayılan KAPALI (`QTSS_AI_AUTO_APPROVE_ENABLED=0`).
- Her AI kararında `prompt_hash` — aynı bağlama tekrar LLM çağırmaz (maliyet + tutarlılık).
- `ai_decisions.meta_json` — token sayısı, model, sürüm; audit trail.

---

## 10. Spawn Sırası

Worker `main.rs` DATABASE_URL bloğu sonundaki spawn sırası (mevcut + yeni AI):

```
tokio::spawn(pnl_rollup_loop)
tokio::spawn(binance_spot_reconcile_loop)
tokio::spawn(binance_futures_reconcile_loop)
tokio::spawn(engine_analysis_loop + confluence_hook)
tokio::spawn(nansen_token_screener_loop)
tokio::spawn(nansen_netflows_loop) ... (7 Nansen loop)
tokio::spawn(setup_scan_engine)
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
strategy_runner::spawn_if_enabled
// --- YENİ: AI Engine ---
tokio::spawn(tactical_layer.run())          // ANTHROPIC_API_KEY varsa
tokio::spawn(operational::run(..))          // ANTHROPIC_API_KEY varsa
tokio::spawn(strategic::run(..))            // + QTSS_AI_STRATEGIC_ENABLED=1
tokio::spawn(expire_stale_decisions_loop)   // 5dk tick, süresi dolmuş kararları temizle
```

---

## 11. Doküman dizini (6 dosya)

`docs/` altında yalnızca aşağıdaki dosyalar tutulur. Cursor ve geliştirme için **birincil kaynak** `QTSS_MASTER_DEV_GUIDE.md` dosyasıdır (`README.md` kısa indeks sağlar).

| Dosya | Rol |
|-------|-----|
| `README.md` | Dizin indeksi |
| `PROJECT.md` | Mimari, crate’ler, API, yol haritası özeti |
| `QTSS_MASTER_DEV_GUIDE.md` | Durum, riskler, iyileştirmeler, AI planı, **FAZ 0–8** |
| `SECURITY.md` | Güvenlik notları |
| `ELLIOTT_V2_STANDARDS.md` | Elliott V2 web referansı |
| `SPEC_EXECUTION_RANGE_SIGNALS_UI.md` | Execution / range / UI şartnamesi |

**Repodan kaldırılan (içerik bu dosyada veya `PROJECT.md` / `.env.example` / kaynak kodda):** `DATA_SOURCES_AND_SOURCE_KEYS.md`, `NANSEN_TOKEN_SCREENER.md`, `SPEC_ONCHAIN_SIGNALS.md`, `PLAN_CONFLUENCE_AND_MARKET_DATA.md`, `QTSS_CURSOR_DEV_GUIDE.md`.

**Not:** `PROJECT.md` içindeki yol haritası, AI engine fazlarıyla uyumlu olacak şekilde zaman içinde güncellenebilir.

---

*Bu doküman projenin tek geliştirme referansıdır. Kod değiştikçe güncellenmelidir.*
