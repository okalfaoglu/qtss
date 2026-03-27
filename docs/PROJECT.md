# QTSS — Proje Dokümanı

Çok borsalı alım-satım ve analiz platformu. Çekirdek **Rust**; web ve mobil tüketim **HTTP API** üzerinden. Bu belge mimari kararları, klasör yapısını ve yol haritasını özetler.

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
├── migrations/                # SQLx migrasyonları (PostgreSQL)
│   ├── 0001_init.sql
│   ├── 0002_oauth.sql
│   ├── 0003_market_catalog.sql
│   ├── 0004_exchange_orders.sql
│   ├── 0005_audit_log.sql
│   ├── 0006_market_bars.sql
│   ├── 0007_acp_chart_patterns.sql
│   ├── 0008_acp_zigzag_seven_fib.sql
│   ├── 0009_acp_pine_indicator_defaults.sql
│   └── 0010_acp_abstract_size_filters.sql
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
    └── qtss-chart-patterns/   # ACP çizim JSON, kanal/inspect/resolve; Pine `find` tam portu değil — bkz. `docs/CHART_PATTERNS_TRENDOSCOPE_PORT.md`
```

**Migrasyonları çalıştırma:** `qtss-api` veya `qtss-worker` başlarken bekleyen `migrations/*.sql` dosyaları otomatik uygulanır: `cargo run -p qtss-api`. İlk kurulumda örnek org + admin + OAuth istemcisi için: `cargo run -p qtss-api --bin qtss-seed` (binary adı `seed` değil, `qtss-seed`).

**Migrasyon checksum uyarısı:** Uygulanmış bir `migrations/*.sql` dosyasını değiştirirseniz SQLx `migration … was previously applied but has been modified` hatası verir. Önce (repo kökünden, `qtss-api` ile aynı `.env`): `cargo run -p qtss-storage --bin qtss-sync-sqlx-checksums` — ardından `cargo run -p qtss-api`. `sqlx-cli` şart değil. Doğru kalıp: eski migrasyonu ellemeden yeni numaralı `.sql` eklemek.

**Planlanan genişlemeler** (henüz ayrı crate olarak yok):

- `crates/qtss-connectors/` — ek borsa adapter’ları (`qtss-binance` yanında)
- `crates/qtss-marketdata/` — WebSocket bar/tick akışı, normalizasyon
- `crates/qtss-analysis/` — teknik analiz motoru (API’de `/analysis/*` iskeleti var)
- `crates/qtss-notify/` — Telegram, e-posta, SMS (API’de `/notify/*` iskeleti var)
- `web/` — **Vite + React** (`npm run dev` / `npm run build`). Grafik: Lightweight Charts; Elliott Wave V2 (`web/src/lib/elliottEngineV2/`). Tez §2.5.3–2.5.5 metin kataloğu tek dosyada: `web/src/lib/elliottRulesCatalog.ts` (panel + `public/elliott_dalga_prensipleri.txt` özeti).

---

## 3. Workspace crate’leri

| Crate | Sorumluluk |
|--------|------------|
| **qtss-common** | `AppMode` (Live / Dry), `tracing` tabanlı log, iş seviyesi (`QtssLogLevel`), kritik olay işaretleme |
| **qtss-domain** | `ExchangeId`, `MarketSegment`, `InstrumentId`, `TimestampBar`, `OrderType` (tam set), copy-trade modelleri, komisyon trait’i, tenancy tipleri |
| **qtss-storage** | Bağlantı havuzu, migrasyon, `app_config`, `pnl_rollups`, `market_bars` (upsert / son barlar) |
| **qtss-execution** | `ExecutionGateway`, `BinanceLiveGateway` (spot/FAPI market+limit + borsa yanıtı) |
| **qtss-backtest** | Senkron backtest motoru, performans metrikleri, walk-forward ve parametre ızgarası optimizasyonu |
| **qtss-reporting** | PDF üretimi (printpdf) |
| **qtss-binance** | Spot / FAPI REST, katalog, `KlineBar`, kapanan mum WS ayrıştırma, komisyon ipucu (`exchangeInfo`) |
| **qtss-api** | Axum + `x-request-id`, `GET /metrics`, IP tabanlı rate limit (`tower-governor`) |
| **qtss-worker** | Heartbeat; `QTSS_KLINE_*` + `DATABASE_URL` ile Binance kline WS ve `market_bars` yazımı |
| **qtss-chart-patterns** | Trend çizgisi bar enterpolasyonu, ACP `patternType` id → isim, `PatternDrawingBatch` (serde), Zigzag iskelesi, kanal altılı tarama API; Pine ile tam özdeş değil — bkz. `docs/CHART_PATTERNS_TRENDOSCOPE_PORT.md` |

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

- **`0001_init.sql`** — `organizations`, `users`, `roles`, `user_roles`, `app_config`, `exchange_accounts`, `copy_subscriptions`, `pnl_rollups`, `backtest_runs`
- **`0002_oauth.sql`** — `oauth_clients`, `refresh_tokens`
- **`0003_market_catalog.sql`** — `exchanges`, `markets`, `instruments` (katalog senkronu)
- **`0004_exchange_orders.sql`** — `exchange_orders` (gönderilen emirler, denetim / idempotency zemini)
- **`0005_audit_log.sql`** — `audit_log` (HTTP mutasyon denetimi)
- **`0006_market_bars.sql`** — `market_bars` (venue OHLCV; worker veya ileride diğer beslemeler)
- **`0007_acp_chart_patterns.sql`** — Pine ACP v6 fabrika: `acp_chart_patterns` (4 zigzag, yalnız ilki açık; `pivot_tail_skip_max: 0`)
- **`0008_acp_zigzag_seven_fib.sql`** — mevcut `acp_chart_patterns` satırını TV fabrikasına hizalar (dosya adı tarihsel)
- **`0009_acp_pine_indicator_defaults.sql`** — eski 7 Fib veya bozuk ayarlar için aynı hizalamayı tekrar uygular
- **`0010_acp_abstract_size_filters.sql`** — `ignore_if_entry_crossed` + `size_filters` (abstractchartpatterns) eksikse ekler

Ayrıntılı güvenlik hedefleri: [SECURITY.md](SECURITY.md).

---

## 8. API (mevcut)

### Kimlik (Bearer)

- `POST /oauth/token` — OAuth 2.0 benzeri: `grant_type` = `password` \| `client_credentials` \| `refresh_token` (gövde JSON veya `application/x-www-form-urlencoded`). Yanıtta `access_token` + `refresh_token`.

`/api/v1/*` uçları **`Authorization: Bearer <access_token>`** ister. JWT içinde `roles` (`roles.key` değerleri) taşınır.

Tüm yanıtlarda **`x-request-id`** (UUID) üretilir veya istemci gönderdiyse yankılanır. **`GET /metrics`** basit Prometheus metni (`qtss_http_requests_total`). **`tower-governor`**: eş IP için jeton kovası (varsayılan ~50 sürdürülebilir RPS; ayar §11).

### Rol matrisi (özet)

| Uç | Gerekli roller |
|----|----------------|
| `GET/POST /api/v1/config`, `DELETE /api/v1/config/{key}` | `admin` |
| `POST /api/v1/reconcile/binance` | `admin` |
| `POST /api/v1/catalog/sync/binance` | `admin`, `trader` |
| `POST /api/v1/market/binance/bars/backfill` | `admin`, `trader` |
| `GET /api/v1/orders/binance` | `admin`, `trader`, `analyst`, `viewer` |
| `POST /api/v1/orders/binance/place`, `POST /api/v1/orders/binance/cancel` | `admin`, `trader` |
| `POST /api/v1/copy-trade/subscriptions`, `PATCH .../{id}/active`, `DELETE .../{id}` | `admin`, `trader` |
| `GET /api/v1/dashboard/pnl`, `GET /api/v1/market/binance/*`, `GET /api/v1/market/bars/recent`, `GET /api/v1/copy-trade/subscriptions`, analysis / notify iskeletleri | `admin`, `trader`, `analyst`, `viewer` |

Yetersiz rol → HTTP **403** (`insufficient_scope`).

### Yollar

- `GET /health` — servis durumu (JWT gerekmez)
- `GET /metrics` — Prometheus uyumlu sayaç (JWT gerekmez; üretimde ağ politikası ile koruyun)
- `GET /api/v1/config` — config listesi
- `POST /api/v1/config` — upsert (`key`, `value`, isteğe bağlı `description`, `actor_user_id`)
- `DELETE /api/v1/config/{key}`
- `GET /api/v1/dashboard/pnl?ledger=&bucket=`
- `POST /api/v1/catalog/sync/binance` — Binance spot + USDT-M enstrüman kataloğunu DB’ye yazar
- `GET /api/v1/market/binance/klines?symbol=&interval=&segment=` — `segment` = `spot` (varsayılan) veya `futures`
- `GET /api/v1/market/binance/commission-defaults?segment=&symbol=` — `symbol` verilirse ilgili `exchangeInfo` çekilir; sembol satırında ücret alanları varsa `source=exchange_info`, yoksa tier0 `fallback_tier0`
- `GET /api/v1/market/binance/stream-urls?symbol=&interval=` — spot ve USDT-M kline WebSocket URL’leri (istemci doğrudan bağlanır)
- `GET /api/v1/market/bars/recent?exchange=&segment=&symbol=&interval=&limit=` — `market_bars` (varsayılan `limit=500`, üst sınır 5000)
- `POST /api/v1/market/binance/bars/backfill` — gövde: `{ "symbol", "interval", "segment?": "spot"|"futures", "limit?": 1..1000 }`; Binance REST klines → `market_bars` upsert (worker olmadan geçmiş dolum)
- `POST /api/v1/reconcile/binance` — Binance **spot** `openOrders` ile yerel `exchange_orders` (`venue_order_id`); JWT kullanıcısının spot API anahtarı gerekir
- `GET /api/v1/orders/binance` — son emirler (kullanıcıya göre, `exchange_orders`)
- `POST /api/v1/orders/binance/place` — gövde: `{ "intent": OrderIntent }`; `exchange_accounts` + `exchange_orders` (`venue_order_id` / `venue_response` Binance yanıtından)
- `POST /api/v1/orders/binance/cancel` — gövde: `client_order_id`, `symbol`, `segment`; yerel kayıt `canceled` olarak işaretlenir
- `GET /api/v1/copy-trade/subscriptions` — `leader` veya `follower` olduğun abonelikler
- `POST /api/v1/copy-trade/subscriptions` — `leader_user_id`, `rule` (JSON, [`CopyRule`](crates/qtss-domain/src/copy_trade.rs) şeması)
- `PATCH /api/v1/copy-trade/subscriptions/{id}/active` — `{ "active": bool }`
- `DELETE /api/v1/copy-trade/subscriptions/{id}`
- `GET /api/v1/analysis/health` — iskelet
- `POST /api/v1/notify/test` — iskelet

`qtss-worker`: `QTSS_KLINE_SYMBOL` (ör. `BTCUSDT`), isteğe bağlı `QTSS_KLINE_INTERVAL` (varsayılan `1m`), `QTSS_KLINE_SEGMENT` (`spot` varsayılan; `futures` / `fapi` / `usdt_futures` → USDT-M WS). **`DATABASE_URL` doluysa** migrasyon + kapanan mumlar `market_bars` tablosuna yazılır; yoksa yalnızca log. Kalıcı çalıştırma: [deploy/README.md](../deploy/README.md) (systemd birim örnekleri).

---

## 9. Güvenlik ve tenancy

- **JWT**: `QTSS_JWT_SECRET` (≥32 byte); `aud` / `iss` doğrulaması açık. İstemci kimlikleri `oauth_clients` tablosunda; refresh token hash ile saklanır.
- API anahtarları (borsa): `exchange_accounts` tablosu (geliştirme); üretimde **Vault/KMS** hedefi. Canlı emir uçları yalnızca ilgili kullanıcı için eşleşen `binance` + `spot`/`futures` satırını kullanır.
- **RBAC**: JWT `roles` + uç bazlı middleware; ayrıntı §8.
- **Rate limit**: `tower-governor`; anahtar = TCP peer veya güvenilen vekil + `X-Forwarded-For` ilk adresi (`QTSS_TRUSTED_PROXIES`). Üretimde ek katman (nginx, cloud WAF) önerilir.
- **Denetim**: `audit_log` tablosu — `/api/v1/*` mutasyonları yalnızca `QTSS_AUDIT_HTTP=1` iken yazılır.
- **Metrikler**: `QTSS_METRICS_TOKEN` doluysa `/metrics` Bearer veya `?token=` ister.
- **Gizli anahtarlar**: `exchange_accounts.api_secret` düz metin — üretimde KMS / Vault ile şifreleme veya harici secret store hedefi.
- Ayrıntılı audit log ve oturum yönetimi sonraki adımlar.
- Çok kiracı: JWT’de `org_id`; şema veya satır düzeyi `org_id` ile genişletme.

---

## 10. Yol haritası

1. **Kimlik ve RBAC** — OAuth + JWT + uç bazlı roller (**kısmen**: token ve RBAC API’de; ince taneli izinler / audit sonra)  
2. **Binance spot + futures** — REST + katalog + kline + stream URL + komisyon fallback + sunucu tarafı kline WS → DB (**kısmen**; hesap bazlı gerçek fee / trade fee API sonra)  
3. **Emir ve mutabakat** — `BinanceLiveGateway` + HTTP place/Cancel; periyodik reconcile (**kısmen**; stub + `qtss-worker`)  
4. **Copy trade** — CRUD API; yürütme kuyruğu ve lead fill dinleme sonra  
5. **Analiz motoru** — ayrı crate + gerçek motor  
6. **AI katmanı** — onay kuyruğu, policy, guardrail  
7. **Bildirim** — `qtss-notify` + kuyruk  
8. **Web UI** — dashboard genişletmesi; grafik/Elliott temel akış mevcut (`web/`)  
9. **Kurumsal** — HA, observability, uyumluluk  

---

## 11. Ortam değişkenleri

Rust ikilileri (`qtss-api`, `qtss-seed`, `qtss-worker`) proje kökündeki `.env` dosyasını başlangıçta yükler (şablon: kökte `.env.example`). Shell’de `export` edilen değerler aynı anahtar için önceliklidir. Üretimde worker/API için systemd `EnvironmentFile` kullanımı: [deploy/README.md](../deploy/README.md).

| Değişken | Açıklama |
|----------|-----------|
| `DATABASE_URL` | PostgreSQL (API zorunlu; worker’da bar yazımı için zorunlu) |
| `QTSS_BIND` | API dinleme adresi (örn. `0.0.0.0:8080`) |
| `QTSS_JWT_SECRET` | HMAC imza anahtarı (**zorunlu**, en az 32 byte) |
| `QTSS_JWT_AUD` | JWT `aud` (varsayılan `qtss-api`) |
| `QTSS_JWT_ISS` | JWT `iss` (varsayılan `qtss`) |
| `QTSS_ACCESS_TTL_SECS` | Access token ömrü saniye (varsayılan 900) |
| `QTSS_REFRESH_TTL_SECS` | Refresh token ömrü saniye (varsayılan 30 gün) |
| `RUST_LOG` / `QTSS_LOG_JSON` | Log seviyesi ve isteğe bağlı JSON satır log |
| `QTSS_KLINE_SYMBOL` | (`qtss-worker`) Doluysa kline WebSocket dinleyicisi açılır |
| `QTSS_KLINE_INTERVAL` | (`qtss-worker`) Mum aralığı (varsayılan `1m`) |
| `QTSS_KLINE_SEGMENT` | (`qtss-worker`) `spot` (varsayılan) veya `futures` / `fapi` / `usdt_futures` |
| `QTSS_RATE_LIMIT_REPLENISH_MS` | Kova yenileme aralığı ms (varsayılan `20` → ~50 sürdürülebilir RPS / IP) |
| `QTSS_RATE_LIMIT_BURST` | Jeton tavanı (varsayılan `120`) |
| `QTSS_TRUSTED_PROXIES` | Güvenilen vekil IP/CIDR listesi; boş = vekil güveni yok; tanımsız = yalnızca loopback vekil |
| `QTSS_METRICS_TOKEN` | Doluysa `/metrics` için `Authorization: Bearer …` veya `?token=` |
| `QTSS_AUDIT_HTTP` | Yalnızca `1` ise mutasyonlar `audit_log`a yazılır; tanımsız veya `1` dışı değer = kapalı |

---

*Son güncelleme: proje deposu ile eşlik eder; kod değiştikçe bu belge gözden geçirilmelidir.*
