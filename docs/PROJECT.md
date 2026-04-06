# QTSS — Proje Dokümanı

Çok borsalı alım-satım ve analiz platformu. Çekirdek **Rust**; web ve mobil tüketim **HTTP API** üzerinden. Bu belge mimari kararları, klasör yapısını ve yol haritasını özetler.

**Hizalama (kesin kural):** Kod ve şablonlar tek doğrulukla güncellenir — tam madde listesi: [QTSS_MASTER_DEV_GUIDE.md](./QTSS_MASTER_DEV_GUIDE.md) — worker `tokio::spawn` + `init_logging` + `run_migrations` ([Bölüm 10 — spawn sırası](./QTSS_MASTER_DEV_GUIDE.md#10-spawn-s%C4%B1ras%C4%B1)); API aynı `init_logging` + migrasyon bağlamı; `data_sources/registry.rs`; `qtss-storage` / `pool.rs`; kök `.env.example`; `migrations/README.md` + [Bölüm 5 — migration kuralları](./QTSS_MASTER_DEV_GUIDE.md#5-migration-kurallar%C4%B1); log seviyeleri [§8.1](./QTSS_MASTER_DEV_GUIDE.md#81-loglama--trace--debug--info--warn--error--critical); [SECURITY.md](./SECURITY.md) worker canlı emir notu; bu belge [§7](#7-veritabanı-özet). **Migrasyon gerçeği (bu depo):** `migrations/0001_qtss_baseline.sql` tek dosya; eski çok adımlı zincir içeride `-- >>> merged from:` ile birleşiktir — ayrıntı [QTSS_MASTER_DEV_GUIDE.md §0.3](./QTSS_MASTER_DEV_GUIDE.md) ve `migrations/README.md`.

---

## İçindekiler

1. [Vizyon ve kapsam](#1-vizyon-ve-kapsam)
2. [Depo yapısı](#2-depo-yapısı)
3. [Workspace crate’leri](#3-workspace-crateleri)
4. [Enum’lar ve config](#4-enumlar-ve-config)
5. [Veri modeli: bar, tick, seans](#5-veri-modeli-bar-tick-seans)
6. [Live ve Dry modlar](#6-live-ve-dry-modlar)
7. [Veritabanı (özet)](#7-veritabanı-özet)
8. [API (mevcut)](#8-api-mevcut)
9. [Güvenlik ve tenancy](#9-güvenlik-ve-tenancy)
10. [Yol haritası](#10-yol-haritası)
11. [Ortam değişkenleri](#11-ortam-değişkenleri)

---

## 1. Vizyon ve kapsam

- **Çok borsa**; varsayılan kripto **Binance**, diğer borsalar adapter ile eklenir. Hisse tarafı (ör. Nasdaq, BIST) ayrı veri ve emir sağlayıcıları ile benzer soyutlama.
- **Spot ve vadeli** öncelikli uygulama; mimaride **tüm emir tipleri** tanımlı, diğer segmentler (marj, opsiyon) sonraya bırakılabilir.
- **Çok sembol**; radar, setup, manuel/otomatik emir; **AI** ile insan onaylı veya (olgunlaştıkça) otonom mod.
- **Copy trade**: lider işlemleri; takipçi çarpanı, slippage, gecikme ve notional limitleri.
- **Analiz**: Elliott ve klasik formasyonlar; analiz **merkezi serviste**; web öncelikle görüntüleme.
- **Bildirim**: Telegram → e-posta, SMS ve diğer kanallar.
- **Tenancy**: şimdilik **tek kurum**; domain’de `OrganizationId` / `TenantContext` ile çok kiracıya genişleme hazır.
- **Regülasyon**: ürün evrimine göre netleştirilecek; tasarımda audit, onay ve risk limitleri ön planda.

---

## 2. Depo yapısı

```
qtss/
├── Cargo.toml                 # Workspace tanımı ve workspace.dependencies
├── rustfmt.toml
├── docs/
│   └── PROJECT.md             # Bu dosya
├── deploy/                    # systemd birim şablonları, worker/API notları
├── migrations/                # SQLx migrasyonları (PostgreSQL). **Bu depo:** yalnızca `0001_qtss_baseline.sql` (+ geçici `0002_*.sql` squash öncesi)
│   ├── README.md              # squash akışı, çift önek yasağı; MASTER §5
│   └── 0001_qtss_baseline.sql # çekirdek + ACP + Nansen + AI + `system_config` + … (`-- >>> merged from:` bölümleri)
├── web/                     # Vite + React + TS; LWC grafik, Elliott V2, ACP kanal taraması (proxy → API)
└── crates/
    ├── qtss-common/           # Uygulama modu (live/dry), merkezi loglama
    ├── qtss-domain/           # Domain tipleri: emir, borsa, bar, copy-trade, vb.
    ├── qtss-storage/          # Postgres, config ve PnL depoları
    ├── qtss-execution/        # Emir yürütme: dry-run, canlı (iskelet), mutabakat iskeleti
    ├── qtss-backtest/         # Backtest motoru, metrikler, optimizasyon iskeleti
    ├── qtss-reporting/        # PDF rapor
    ├── qtss-binance/          # Binance spot + USDT-M REST, katalog senkronu, kline ayrıştırma
    ├── qtss-api/              # Axum HTTP API (OAuth, RBAC, piyasa uçları)
    ├── qtss-worker/           # Arka plan: heartbeat; isteğe bağlı kline WS → market_bars
    ├── qtss-nansen/           # Nansen HTTP (screener, smart-money, TGM, `tgm/perp-pnl-leaderboard`, `profiler/perp-positions`)
    ├── qtss-notify/           # Telegram, webhook, … (`POST /notify/test` + worker uyarıları)
    ├── qtss-strategy/         # Dry strateji döngüleri (signal_filter, whale_momentum, risk, …)
    ├── qtss-analysis/         # `engine_symbols` → trading_range + signal_dashboard snapshot döngüsü (worker spawn)
    ├── qtss-reconcile/        # Binance açık emir yamaları (`qtss-api` + worker reconcile döngüleri)
    ├── qtss-ai/               # LLM sağlayıcıları, AI kararları, worker `ai_engine` entegrasyonu
    ├── qtss-signal-card/      # Sinyal kartı yardımcıları
    ├── qtss-indicators/       # Gösterge hesapları
    ├── qtss-tbm/              # TBM (dashboard) destek crate’i
    ├── qtss-telegram-setup-analysis/  # Telegram setup analizi (API webhook + worker ile ilişkili)
    └── qtss-chart-patterns/   # ACP çizim JSON, kanal tarama; Pine tam parity değil — bkz. [ACP_PINE_PARITY_FIX.md](./ACP_PINE_PARITY_FIX.md)
```

**Migrasyonları çalıştırma:** `qtss-api` veya `qtss-worker` başlarken bekleyen `migrations/*.sql` dosyaları otomatik uygulanır: `cargo run -p qtss-api`. İlk kurulumda örnek org + admin + OAuth istemcisi için: `cargo run -p qtss-api --bin qtss-seed` (binary adı `seed` değil, `qtss-seed`).

**Varsayılan log (API + worker):** Kod içi `init_logging` her iki ikilikte de `sqlx::postgres::notice=warn` içerir (`_sqlx_migrations` Postgres NOTICE’larını INFO’da göstermez) — [QTSS_MASTER_DEV_GUIDE.md §8.1](./QTSS_MASTER_DEV_GUIDE.md#81-loglama--trace--debug--info--warn--error--critical).

**Migrasyon checksum uyarısı:** Uygulanmış bir `migrations/*.sql` dosyasını değiştirirseniz SQLx `migration … was previously applied but has been modified` hatası verir. Önce (repo kökünden, `qtss-api` ile aynı `.env`): `cargo run -p qtss-storage --bin qtss-sync-sqlx-checksums` — ardından `cargo run -p qtss-api`. `sqlx-cli` şart değil. Doğru kalıp: eski migrasyonu ellemeden yeni numaralı `.sql` eklemek — [QTSS_MASTER_DEV_GUIDE.md §5](./QTSS_MASTER_DEV_GUIDE.md#5-migration-kurallar%C4%B1).

**Tekil sürüm numarası:** Aynı `NNNN_` öneki iki dosyada kullanılamaz (`_sqlx_migrations` satır başına tek checksum). Dağıtımda genelde yalnız **`migrations/0001_qtss_baseline.sql`** vardır (içinde `-- >>> merged from: …` bölümleri). Yeni delta: geçici **`0002_*.sql`** + `python3 scripts/squash_migrations_into_one.py` ile tekrar tek dosya — `migrations/README.md`. `qtss-sync-sqlx-checksums` çift öneği reddeder.

**Nansen setup scan:** Tablolar `nansen_setup_runs` / `nansen_setup_rows` (baseline içinde); worker `setup_scan_engine` (`QTSS_SETUP_SCAN_SECS`, `QTSS_SETUP_MAX_SNAPSHOT_AGE_SECS`); varsayılan `QTSS_SETUP_SNAPSHOT_ONLY=1` ile setup Nansen’e ikinci HTTP göndermez (kredi yalnız `nansen_engine`); TP1 için 1h OHLC + 6h vol; `meta_json.spec_version` (ör. `nansen_setup_v3`); 5 LONG + 5 SHORT; API `GET /api/v1/analysis/nansen/setups/latest`.

**Nansen extended HTTP:** `nansen_extended.rs` + `nansen_engine` re-export; `data_snapshots` anahtarları ve TGM **perp PnL leaderboard** → `app_config.nansen_whale_watchlist` → whale aggregate (`profiler/perp-positions`). Varsayılan leaderboard yolu: `api/v1/tgm/perp-pnl-leaderboard` (`NANSEN_PERP_LEADERBOARD_PATH`). Tam env listesi: kök `.env.example`, özet: [QTSS_MASTER_DEV_GUIDE.md §6](./QTSS_MASTER_DEV_GUIDE.md#6-ortam-de%C4%9Fi%C5%9Fkenleri).

**Planlanan genişlemeler** (henüz ayrı crate olarak yok veya kısmi):

- `crates/qtss-connectors/` — tek çatı altında çok borsa adapter’ı (API’de şimdilik `orders_bybit` / `orders_okx` yazma uçları var; ayrı crate yok)
- `crates/qtss-marketdata/` — venue-bağımsız WebSocket bar/tick normalizasyonu (Binance kline WS şu an `qtss-worker` + `qtss-binance` içinde)

- Mevcut analiz: `crates/qtss-analysis/` — snapshot döngüsü (worker); API `/analysis/*` geniş yüzey (`routes/analysis.rs` vb.).
- `web/` — **Vite + React** (`npm run dev` / `npm run build`). Grafik: Lightweight Charts; Elliott Wave V2 (`web/src/lib/elliottEngineV2/`). Tez §2.5.3–2.5.5 metin kataloğu tek dosyada: `web/src/lib/elliottRulesCatalog.ts` (panel + `public/elliott_dalga_prensipleri.txt` özeti).

Mevcut ayrı crate’ler: `qtss-nansen`, `qtss-notify`, `qtss-strategy`, `qtss-analysis` — bkz. §3 tablo ve [QTSS_MASTER_DEV_GUIDE.md](./QTSS_MASTER_DEV_GUIDE.md) (durum özeti §0, FAZ tabloları §4).

---

## 3. Workspace crate’leri

| Crate | Sorumluluk |
|--------|------------|
| **qtss-common** | `AppMode` (Live / Dry), `tracing` tabanlı log (`init_logging`, `log_business`, `log_critical`), `QtssLogLevel` — **seviye politikası:** [QTSS_MASTER_DEV_GUIDE.md §8.1](./QTSS_MASTER_DEV_GUIDE.md#81-loglama--trace--debug--info--warn--error--critical) |
| **qtss-domain** | `ExchangeId`, `MarketSegment`, `InstrumentId`, `TimestampBar`, `OrderType` (tam set), copy-trade modelleri, komisyon trait’i, tenancy tipleri |
| **qtss-storage** | Bağlantı havuzu, migrasyon, `app_config`, `engine_symbols`, `data_snapshots`, `onchain_signal_scores`, `market_bars`, paper, Nansen setup, … |
| **qtss-execution** | `ExecutionGateway`, `BinanceLiveGateway`, `DryRunGateway`; `set_reference_price` (paper) |
| **qtss-reconcile** | Binance spot/futures açık emir listesi ↔ `exchange_orders` yama mantığı |
| **qtss-nansen** | Nansen HTTP istemcisi (token screener, smart-money, TGM — `perp-pnl-leaderboard` göreli yol — profiler perp-positions) |
| **qtss-notify** | Telegram / webhook / … bildirim gönderimi |
| **qtss-strategy** | `signal_filter`, `whale_momentum`, `arb_funding`, `copy_trade`, `risk` — worker `strategy_runner` ile dry |
| **qtss-backtest** | Senkron backtest motoru, performans metrikleri, walk-forward ve parametre ızgarası optimizasyonu |
| **qtss-reporting** | PDF üretimi (printpdf) |
| **qtss-binance** | Spot / FAPI REST, katalog, `KlineBar`, kapanan mum WS ayrıştırma, komisyon ipucu (`exchangeInfo`) |
| **qtss-api** | Axum + `x-request-id`, `GET /metrics`, IP tabanlı rate limit (`tower-governor`) |
| **qtss-analysis** | `engine_analysis_loop`, trading range / confluence / snapshot üretimi (worker) |
| **qtss-worker** | Yukarıdaki spawn zinciri ([MASTER §10](./QTSS_MASTER_DEV_GUIDE.md#10-spawn-s%C4%B1ras%C4%B1)): katalog senkronu, reconcile döngüleri, Nansen (çoklu loop), setup scan, harici motorlar, on-chain skor, notify, kill switch, position manager, copy trade, strateji, AI, user stream; kline WS önceliği: etkin `engine_symbols` (Binance) satırları → yoksa `QTSS_KLINE_SYMBOL(S)` / env |
| **qtss-ai** | Çoklu LLM sağlayıcı, `ai_decisions` / direktifler, worker `ai_engine` |
| **qtss-signal-card** | Sinyal kartı veri/yardımcı |
| **qtss-indicators** | Teknik gösterge hesapları |
| **qtss-tbm** | TBM paneli desteği |
| **qtss-telegram-setup-analysis** | Telegram setup analizi (API status ucu + webhook path’leri) |
| **qtss-chart-patterns** | ACP çizim JSON, kanal altılı tarama; Pine tam parity değil — [ACP_PINE_PARITY_FIX.md](./ACP_PINE_PARITY_FIX.md) |

---

## 4. Enum’lar ve config

- **Enum tanımları** yalnızca **Rust kodunda** (`qtss-domain` vb.): derleme zamanı güvenliği, eksiksiz `match`.
- **PostgreSQL `app_config`** ve diğer tablolar: enum **değerlerini** string veya JSON olarak saklar; API ve worker katmanı `serde` ile doğrular.
- **Sık değişen parametreler** (komisyon fallback, risk limiti, feature flag): config tablosunda **sayı/string**; iş kuralı yine kodda.

Bu ayrım, iş kurallarının tek kaynağının kod kalmasını sağlar; veritabanı ise dağıtılmış **durum ve parametre** deposudur.

---

## 5. Veri modeli: bar, tick, seans

- **Birincil iş birimi** (şu an): **zaman damgalı OHLCV barları**. Strateji ve backtest bu üzerinden.
- **Tick**: ileride ayrı akış ve depolama; motorlar aşamalı olarak genişletilir.
- **7/24 süreç vs borsa saatleri**: kripto sürekli işlem eğilimindeyken, hisse gibi venue’lerde **seans / tatil / bakım** vardır. İç mantık **UTC**; “şu an işlem var mı?” sorusu için:
  - **Takvim / seans** (venue + enstrüman),
  - **Akış sağlığı** (beklenen veri gelmedi → kopukluk alarmı)  
  ayrı ele alınmalıdır; sessizlik her zaman “kapalı piyasa” anlamına gelmez.

---

## 6. Live ve Dry modlar

| Mod | Veri | Emir | Defter |
|-----|------|------|--------|
| **Live** | Canlı (veya üretim veri yolu) | Gerçek borsa emirleri | Gerçek sonuçların kaydı |
| **Dry** | Canlı veri ile aynı besleme (tasarım hedefi) | Sanal bakiye / simüle doldurma | Ayrı ledger kayıtları (şema/ledger alanı ile ayrım) |

`qtss-common::AppMode` ve `DbPersistenceMode` bu ayrımı ifade eder.

---

## 7. Veritabanı (özet)

**Tek migrasyon dosyası:** `migrations/0001_qtss_baseline.sql`. Tarihsel çok dosyalı zincir (`0001_init` … `0012_acp_*`, sonrası Nansen/AI/`system_config` vb.) bu dosyada **`-- >>> merged from: …`** bölümleriyle birleşiktir; şema özeti (kanıt: aynı dosyada `CREATE TABLE`):

- **Kimlik / org:** `organizations`, `users`, `roles`, `user_roles`, `user_permissions`, `oauth_clients`, `refresh_tokens`
- **Config:** `app_config`, `system_config`
- **Katalog / bar meta:** `exchanges`, `markets`, `instruments`, `bar_intervals`, `market_bars`
- **Emir / defter:** `exchange_accounts`, `exchange_orders`, `exchange_fills`, `copy_subscriptions`, `copy_trade_execution_jobs`, paper: `paper_balances`, `paper_fills`, `range_signal_paper_executions`
- **PnL / backtest:** `pnl_rollups`, `backtest_runs`
- **Denetim:** `audit_log`
- **Analiz / motor:** `engine_symbols`, `engine_symbol_ingestion_state`, `analysis_snapshots`, `range_signal_events`, `data_snapshots`, `market_confluence_snapshots`, `external_data_sources`
- **Nansen:** `nansen_snapshots`, `nansen_setup_runs`, `nansen_setup_rows`
- **On-chain:** `onchain_signal_scores`
- **ACP:** `app_config` anahtarı `acp_chart_patterns` (fabrika JSON; baseline içi `0007`–`0012` birleşimi)
- **Bildirim / AI:** `notify_outbox`, `ai_approval_requests`, `ai_decisions`, `ai_tactical_decisions`, `ai_position_directives`, `ai_portfolio_directives`, `ai_decision_outcomes`

Yeni DDL: geçici `0002_*.sql` → squash — [migrations/README.md](../migrations/README.md), [QTSS_MASTER_DEV_GUIDE.md §5](./QTSS_MASTER_DEV_GUIDE.md#5-migration-kurallar%C4%B1).

Ayrıntılı güvenlik hedefleri: [SECURITY.md](SECURITY.md).

---

## 8. API (mevcut)

### Kimlik (Bearer)

- `POST /oauth/token` — OAuth 2.0 benzeri: `grant_type` = `password` \| `client_credentials` \| `refresh_token` (gövde JSON veya `application/x-www-form-urlencoded`). Yanıtta `access_token` + `refresh_token`.

`/api/v1/*` uçları **`Authorization: Bearer <access_token>`** ister. JWT içinde `roles` (`roles.key` değerleri) ve ilk aşama `permissions` (`qtss:read` \| `qtss:ops` \| `qtss:admin` \| `qtss:audit:read`, rol haritasından) taşınır; `permissions` boş eski belirteçlerde API isteği sırasında `roles` ile tamamlanır. `user_permissions` tablosundaki satırlar aynı istekte birleştirilir (`QTSS_JWT_MERGE_DB_PERMISSIONS=0` ile kapatılabilir). Erişim middleware’leri bu etkin izinlere göre çalışır.

Tüm yanıtlarda **`x-request-id`** (UUID) üretilir veya istemci gönderdiyse yankılanır. **`GET /metrics`** basit Prometheus metni (`qtss_http_requests_total`). **`tower-governor`**: eş IP için jeton kovası (varsayılan ~50 sürdürülebilir RPS; ayar §11).

### Rol matrisi (özet)

| Uç | Gerekli roller |
|----|----------------|
| `GET/POST /api/v1/config`, `GET /api/v1/config/{key}`, `DELETE /api/v1/config/{key}` | `admin` |
| `GET/PUT /api/v1/users/{user_id}/permissions` | `admin` (hedef kullanıcı aynı `org_id`) |
| `GET /api/v1/audit/recent` | `qtss:admin` veya `qtss:audit:read` (JWT / `user_permissions`) |
| `POST /api/v1/reconcile/binance`, `POST /api/v1/reconcile/binance/futures` | `admin` |
| `POST /api/v1/catalog/sync/binance` | `admin`, `trader` |
| `POST /api/v1/market/binance/bars/backfill` | `admin`, `trader` |
| `GET /api/v1/orders/binance` | `admin`, `trader`, `analyst`, `viewer` |
| `POST /api/v1/orders/binance/place`, `POST /api/v1/orders/binance/cancel` | `admin`, `trader` |
| `POST /api/v1/copy-trade/subscriptions`, `PATCH .../{id}/active`, `DELETE .../{id}` | `admin`, `trader` |
| `GET /api/v1/dashboard/pnl`, `GET /api/v1/dashboard/pnl/equity`, `GET /api/v1/market/binance/*`, `GET /api/v1/market/bars/recent`, `GET /api/v1/copy-trade/subscriptions`, `GET /api/v1/fills`, `GET /api/v1/ai/*` (okuma), `GET /api/v1/notify/outbox`, `GET /api/v1/analysis/*` (okuma), `GET /api/v1/catalog/*` (salt okunur katalog) | `admin`, `trader`, `analyst`, `viewer` |
| `POST /api/v1/dashboard/pnl/rebuild`, `GET|POST /api/v1/admin/system-config`, `POST /api/v1/admin/kill-switch/reset`, `POST /api/v1/ai/decisions/{id}/approve` … | `admin` |
| `POST /api/v1/notify/outbox` | `admin`, `trader` |
| `POST /api/v1/ai/approval-requests` | `admin`, `trader` |
| `PATCH /api/v1/ai/approval-requests/{id}` | `admin` |

Yetersiz rol → HTTP **403** (`insufficient_scope`). Tam ağaç: `crates/qtss-api/src/routes/mod.rs` (`api_router`). **Not:** `GET /ai/decisions`, `GET /ai/directives/*` rotaları bu dosyada `require_dashboard_roles` ile sarılmamıştır (yalnızca geçerli JWT); diğer çoğu `/api/v1/*` ucu dashboard rolleriyle kısıtlıdır.

### Yollar (kodla eşleşen özet)

**JWT gerekmez:** `GET /health`, `GET /live`, `GET /ready`, `GET /metrics`, `POST /oauth/token`, `GET /api/v1/locales`, `GET /api/v1/bootstrap/web-oauth-client`, Telegram webhook path’leri (`/telegram/webhook/{secret}`, `/telegram/setup-analysis/{secret}` — `main.rs`).

**`/api/v1` (Bearer):** Oturum: `GET /me`, `PATCH /me/locale`. İzinler: `GET|PUT /users/{user_id}/permissions`. Denetim: `GET /audit/recent`. Config: `GET|POST /config`, `GET /config/{key}`, `DELETE /config/{key}`; admin: `GET|POST /admin/system-config`, `GET|DELETE /admin/system-config/{module}/{key}`, `POST /admin/kill-switch/reset`. Dashboard: `GET /dashboard/pnl`, `GET /dashboard/pnl/equity`, `POST /dashboard/pnl/rebuild`. Katalog: `GET /catalog/exchanges|markets|instruments|bar-intervals`, yazma: `POST /catalog/...`; `POST /catalog/sync/binance`. Piyasa: `GET /market/binance/klines|commission-defaults|stream-urls|commission-account`, `GET /market/bars/recent`, `POST /market/binance/bars/backfill`. Mutabakat: `POST /reconcile/binance`, `POST /reconcile/binance/futures`. Emirler: Binance `GET /orders/binance`, `GET /orders/binance/fills`, `POST …/place|cancel`; Bybit/OKX `POST /orders/bybit|okx/place|cancel`; paper `GET /orders/dry/fills|balance`, `POST /orders/dry/place`. Birleşik fill listesi: `GET /fills`. Copy trade: `GET|POST /copy-trade/subscriptions`, `PATCH|DELETE …/{id}…`. Backtest: `POST /backtest/run`. Bildirim: `POST /notify/test`, `GET|POST /notify/outbox`. AI onay: `GET|POST /ai/approval-requests`, `PATCH /ai/approval-requests/{id}`. AI kararlar: `GET /ai/decisions`, `GET /ai/decisions/{id}`, `GET /ai/directives/tactical|portfolio`; admin `DELETE /ai/decisions?status=error` (toplu hata satırı silme), `POST /ai/decisions/{id}/approve|reject`, `POST …/link-approval-request`. Analiz: `GET /analysis/health`, `GET /analysis/chart-patterns-config`, `GET /analysis/elliott-wave-config`, `POST /analysis/patterns/channel-six`, `GET /analysis/engine/symbols|snapshots|ingestion-state`, `GET /analysis/engine/confluence/latest`, `GET /analysis/confluence/latest`, `GET /analysis/data-snapshots`, `GET /analysis/market-context/latest|summary`, `GET /analysis/market-confluence/history`, `GET /analysis/engine/range-signals`, `GET /analysis/range-engine/config`, `GET /analysis/nansen/snapshot`, `GET /analysis/nansen/setups/latest`, `GET /analysis/onchain-signals/latest|history|breakdown`, `GET /analysis/external-fetch/sources|snapshots|snapshots/{key}`; yazma: `POST /analysis/engine/symbols-bulk`, `POST /analysis/engine/symbols`, `PATCH /analysis/engine/symbols/{id}`, `PATCH /analysis/range-engine/config`, `POST|DELETE /analysis/external-fetch/sources…`. Telegram setup analizi: `GET /telegram-setup-analysis/status`.

`qtss-worker`: **`DATABASE_URL` doluysa** migrasyon, arka plan döngüleri ve (Binance için) önce etkin `engine_symbols` satırlarından kline WebSocket; yoksa `QTSS_KLINE_SYMBOL` / `QTSS_KLINE_SYMBOLS` / `worker.kline_symbols_csv` ile birleşik veya tek sembol akışı (`crates/qtss-worker/src/main.rs`). `QTSS_MARKET_DATA_EXCHANGE` (varsayılan `binance`). Ortam: kök `.env.example`, [QTSS_MASTER_DEV_GUIDE.md §6](./QTSS_MASTER_DEV_GUIDE.md#6-ortam-de%C4%9Fi%C5%9Fkenleri). Kalıcı çalıştırma: [deploy/README.md](../deploy/README.md).

---

## 9. Güvenlik ve tenancy

- **JWT**: `QTSS_JWT_SECRET` (≥32 byte); `aud` / `iss` doğrulaması açık. İstemci kimlikleri `oauth_clients` tablosunda; refresh token hash ile saklanır.
- API anahtarları (borsa): `exchange_accounts` tablosu (geliştirme); üretimde **Vault/KMS** hedefi. Canlı emir uçları yalnızca ilgili kullanıcı için eşleşen `binance` + `spot`/`futures` satırını kullanır.
- **RBAC**: JWT `roles` + uç bazlı middleware; ayrıntı §8.
- **Rate limit**: `tower-governor`; anahtar = TCP peer veya güvenilen vekil + `X-Forwarded-For` ilk adresi (`QTSS_TRUSTED_PROXIES`). Üretimde ek katman (nginx, cloud WAF) önerilir.
- **Denetim**: `audit_log` tablosu — `/api/v1/*` mutasyonları yalnızca `QTSS_AUDIT_HTTP=1` iken yazılır.
- **Metrikler**: `QTSS_METRICS_TOKEN` doluysa `/metrics` Bearer veya `?token=` ister; metriklerde `qtss_build_info` + HTTP sayacı.
- **Probe**: API — `GET /live`, `GET /ready`, `GET /health`. Worker — `QTSS_WORKER_HTTP_BIND` ile aynı `/live` ve `/ready` (DB yoksa `ready` içinde `database: none`).
- **Gizli anahtarlar**: `exchange_accounts.api_secret` düz metin — üretimde KMS / Vault ile şifreleme veya harici secret store hedefi.
- Ayrıntılı audit log ve oturum yönetimi sonraki adımlar.
- Çok kiracı: JWT’de `org_id`; şema veya satır düzeyi `org_id` ile genişletme.

---

## 10. Yol haritası

**Teknik şartname (işlem modları, range sinyalleri, grafik işaretleri, dashboard):** [SPEC_EXECUTION_RANGE_SIGNALS_UI.md](./SPEC_EXECUTION_RANGE_SIGNALS_UI.md)  
**F7 — Çok kaynaklı piyasa verisi, confluence, bildirimler, İngilizce alan adları:** Ayrı `PLAN_CONFLUENCE_AND_MARKET_DATA.md` dosyası kaldırılmıştır; plan ve kaynak özeti [QTSS_MASTER_DEV_GUIDE.md](./QTSS_MASTER_DEV_GUIDE.md) (Bölüm 0–4, FAZ tabloları) ile bu belgenin yol haritası maddelerinde birleştirilmiştir.

1. **Kimlik ve RBAC** — OAuth + JWT + `qtss:audit:read` + `user_permissions` + admin izin CRUD; `GET /api/v1/audit/recent` (admin veya salt okunur denetim izni); `PUT .../permissions` + `audit_log.details` (`QTSS_AUDIT_HTTP=1`) (**kısmen**; kurumlar arası admin, tam olay şeması sonra)  
2. **Binance spot + futures** — REST + katalog + kline + stream URL + komisyon fallback + kline WS → DB; hesap bazlı fee: `commission-account` (**v1 tamam**)  
3. **Emir ve mutabakat** — `BinanceLiveGateway` + HTTP place/Cancel; `qtss-reconcile` + worker/API reconcile; `*_PATCH_STATUS`, isteğe bağlı `*_REFINE_ORDER_STATUS` / `*_REFINE_MAX` (`GET .../order` → `filled` / `canceled` / `partially_filled`); kalan `submitted` → `reconciled_not_open`. **User stream (isteğe bağlı):** `binance_user_stream.rs` + `QTSS_BINANCE_USER_STREAM_ENABLED` → fill / order durumu DB; reconcile ile birlikte kullanılabilir (**kısmen**: venue/edge durumları ve ince ayar)  
4. **Copy trade** — CRUD API + `copy_trade_follower` + `copy_trade_queue` / `copy_trade_execution_jobs` (**kısmen**); dolum hızı için user stream + DB üzerinden pozisyon/ledger tutarlılığı; canlı follower emir mantığı / SLA ayrı iterasyon  
5. **Analiz motoru** — `qtss-analysis` crate + worker entegrasyonu (**kısmen**; genişletilmiş motor / tam API yüzeyi sonra)  
6. **AI katmanı** — onay kuyruğu DB + HTTP (**kısmen**; worker/LLM/policy sonraki)  
7. **Bildirim** — `qtss-notify` + `notify_outbox` worker (**kısmen**; retry/DLQ/çoklu tüketici sonra)  
8. **Web UI** — grafik/Elliott temel akış + çekmece **AI → Outbox ve onay** alt sekmesi (`notify_outbox` + `ai_approval_requests`, `permissions` özeti) (**kısmen**; PnL grafikleri / emir defteri / audit UI sonra)  
9. **Kurumsal** — API `/live` + `/ready` + `/metrics` + worker `QTSS_WORKER_HTTP_BIND` probe (**kısmen**); çok örnek HA, uyumluluk raporları sonra  
10. **CI** — Repodaki `.github/workflows/*.yml` (ör. `ci.yml`): workspace `cargo check` / `cargo test`, `web` için `npm ci` + derleme / i18n kontrolleri; tanıma göre Postgres migrasyon job’u (**kısmen**; eksik job’lar sonraki iterasyon)  

---

## 11. Ortam değişkenleri

Rust ikilileri (`qtss-api`, `qtss-seed`, `qtss-worker`) proje kökündeki `.env` dosyasını başlangıçta yükler (şablon: kökte `.env.example`). **Web (Vite)** şablonu: `web/.env.example` — yalnız istemci OAuth/grafik; worker/Nansen ile karıştırılmaz (**kesin kural:** [QTSS_MASTER_DEV_GUIDE.md](./QTSS_MASTER_DEV_GUIDE.md) §0 ve §6). Shell’de `export` edilen değerler aynı anahtar için önceliklidir. Üretimde worker/API için systemd `EnvironmentFile` kullanımı: [deploy/README.md](../deploy/README.md). Worker/Nansen/strateji anahtarlarının tam listesi: aynı belge **[§6](./QTSS_MASTER_DEV_GUIDE.md#6-ortam-de%C4%9Fi%C5%9Fkenleri)**; migrasyon numaraları **[§5](./QTSS_MASTER_DEV_GUIDE.md#5-migration-kurallar%C4%B1)**; spawn sırası **[§10](./QTSS_MASTER_DEV_GUIDE.md#10-spawn-s%C4%B1ras%C4%B1)**; varsayılan log **[§8.1](./QTSS_MASTER_DEV_GUIDE.md#81-loglama--trace--debug--info--warn--error--critical)**; FAZ / dosya eşlemesi **§4**.

| Değişken | Açıklama |
|----------|-----------|
| `DATABASE_URL` | PostgreSQL (API zorunlu; worker’da bar yazımı için zorunlu) |
| `QTSS_BIND` | API dinleme adresi (örn. `0.0.0.0:8080`) |
| `QTSS_JWT_SECRET` | HMAC imza anahtarı (**zorunlu**, en az 32 byte) |
| `QTSS_JWT_AUD` | JWT `aud` (varsayılan `qtss-api`) |
| `QTSS_JWT_ISS` | JWT `iss` (varsayılan `qtss`) |
| `QTSS_ACCESS_TTL_SECS` | Access token ömrü saniye (varsayılan 900) |
| `QTSS_REFRESH_TTL_SECS` | Refresh token ömrü saniye (varsayılan 30 gün) |
| `RUST_LOG` / `QTSS_LOG_JSON` | Tanımlıysa kod içi varsayılan log filtresini override eder; varsayılanlar [QTSS_MASTER_DEV_GUIDE.md §8.1](./QTSS_MASTER_DEV_GUIDE.md#81-loglama--trace--debug--info--warn--error--critical) — JSON satır log: `QTSS_LOG_JSON=1` ([§6](./QTSS_MASTER_DEV_GUIDE.md#6-ortam-de%C4%9Fi%C5%9Fkenleri)) |
| `QTSS_KLINE_SYMBOL` | (`qtss-worker`) Doluysa kline WebSocket dinleyicisi açılır |
| `QTSS_KLINE_INTERVAL` | (`qtss-worker`) Mum aralığı (varsayılan `1m`) |
| `QTSS_KLINE_SEGMENT` | (`qtss-worker`) `spot` (varsayılan) veya `futures` / `fapi` / `usdt_futures` |
| `QTSS_WORKER_HTTP_BIND` | (`qtss-worker`) Doluysa `host:port` üzerinde `/live` ve `/ready` (kube probe); tanımsız = kapalı |
| `QTSS_RATE_LIMIT_REPLENISH_MS` | Kova yenileme aralığı ms (varsayılan `20` → ~50 sürdürülebilir RPS / IP) |
| `QTSS_RATE_LIMIT_BURST` | Jeton tavanı (varsayılan `120`) |
| `QTSS_TRUSTED_PROXIES` | Güvenilen vekil IP/CIDR listesi; boş = vekil güveni yok; tanımsız = yalnızca loopback vekil |
| `QTSS_METRICS_TOKEN` | Doluysa `/metrics` için `Authorization: Bearer …` veya `?token=` |
| `QTSS_AUDIT_HTTP` | Yalnızca `1` ise mutasyonlar `audit_log`a yazılır; tanımsız veya `1` dışı değer = kapalı |

---

*Son güncelleme: proje deposu ile eşlik eder; kod değiştikçe bu belge gözden geçirilmelidir.*
