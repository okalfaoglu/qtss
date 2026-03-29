# Teknik şartname — İşlem modları, range sinyalleri, grafik işaretleri ve dashboard

Bu belge, algoritmik işlem motorunun **çalışma modlarını**, **ortak finansal kurallarını**, **range trading sinyal verisinin** veritabanına aktarımını ve **web arayüzünde** mum grafiği ile dashboard gösterimini tek çatı altında tanımlar. Kod referansları: `qtss-domain::execution`, `qtss-domain::commission`, `qtss-execution`, `qtss-backtest`, `qtss-worker` (motor snapshot’ları), `web/` (grafik ve panel).

---

## 1. Amaç ve kapsam

| Bölüm | Amaç |
|--------|------|
| 2–3 | Live / Dry / Backtest modlarının tanımı ve sınırları |
| 4 | Komisyon ve sanal bakiye gibi tüm modlarda geçerli kurallar |
| 5 | Range tabanlı sinyal üretimi → DB şeması ve olay modeli (hedef) |
| 6 | Mum grafiği: giriş/çıkış ve açık pozisyon gösterimi (hedef) |
| 7 | Dashboard’ta sinyal özetinin sunumu (mevcut kısmen, hedef genişletme) |
| 8–9 | As-is / to-be, izlenebilirlik ve uygulama fazları |

Bu belge **ürün gereksinimleri + mimari sözleşme** niteliğindedir; API söz dizimi ve tablo DDL’si ayrı migrasyon / OpenAPI dokümanları ile güncellenir.

---

## 2. Sistem çalışma modları (Execution Modes)

Algoritmik işlem motoru, farklı test ve canlı operasyon ihtiyaçlarını karşılamak üzere **üç temel modda** çalışacak şekilde tasarlanmıştır. Mod seçimi, veri kaynağı, emir yürütme kanalı ve muhasebe kayıtlarının bağlayıcıdır. Rust tarafında tip karşılığı: `qtss_domain::execution::ExecutionMode` (`live`, `dry`, `backtest`).

### 2.1 Live (canlı işlem modu)

- **Veri akışı:** Borsadan (ör. Binance; ileride BIST vb.) **anlık ve gerçek zamanlı** piyasa verisi alınır.
- **İşlem yürütme:** Üretilen alım/satım sinyalleri **gerçek emirler** olarak doğrudan borsaya iletilir; **gerçek bakiye** kullanılır. Uygulama gateway’i: `qtss_execution::BinanceLiveGateway` (ve ilgili borsa adapter’ları).
- **Kayıt:** Gerçekleşen işlemler, emir yaşam döngüsü, mutabakat ve portföy hareketleri **denetim ve analiz** için veritabanına işlenir (`exchange_orders` ve türev tablolar; politika tenant bazlı genişletilebilir).

### 2.2 Dry (canlı simülasyon / paper trading)

- **Veri akışı:** Borsadan **Live ile aynı** canlı akış kullanılır.
- **İşlem yürütme:** Sinyaller borsaya **gönderilmez**; motor içinde **sanal bakiye** üzerinden simüle edilir. `qtss_execution::apply_place` + `DryRunGateway`: Market/Limit, `CommissionPolicy`, yetersiz quote/base için `InsufficientPaper`.
- **Kayıt:** `paper_balances` (kullanıcı başına quote + taban pozisyonlar JSONB) ve append-only `paper_fills`; API: `POST /api/v1/orders/dry/place`, `GET /api/v1/orders/dry/balance`, `GET /api/v1/orders/dry/fills` (`migrations/0017_paper_ledger.sql`).

### 2.3 Backtest (geçmiş veri testi)

- **Veri akışı:** Belirlenen tarih aralığında **geçmiş bar / OHLC** (ve ileride tick) verisi kullanılır (`market_bars` veya dış dosya/akış).
- **İşlem yürütme:** Algoritma geçmiş üzerinde **hızlı replay** ile çalışır; işlemler yine **sanal bakiye** ile simüle edilir. Çekirdek motor: `qtss_backtest::BacktestEngine`.
- **Kayıt ve amaç:** İşlem listesi, equity eğrisi, **maksimum düşüş (drawdown)**, kâr/zarar ve diğer performans metrikleri üretilir; strateji ve kod kalitesinin ölçülmesi hedeflenir (`PerformanceReport` ve genişletmeler).

---

## 3. Modlar arası tutarlılık

- Aynı **strateji arayüzü** (sinyal üretimi) mümkün olduğunca üç modda da **aynı mantık** ile çalışır; yalnızca veri beslemesi ve `ExecutionGateway` implementasyonu değişir.
- **Range trading** analiz katmanı (Bölüm 5) şu an `analysis_snapshots` / `signal_dashboard` ile **gözlemlenebilirlik** sağlar; **otomatik emir** üretimi Live/Dry/backtest strateji döngüsüne bağlandığında mod kuralları Bölüm 2’ye tabidir.

---

## 4. Ortak kurallar ve finansal hesaplamalar

Stratejilerin gerçekçi sonuç üretmesi için **tüm modlarda** aşağıdaki kurallar uygulanır. Politika tipi: `qtss_domain::commission::CommissionPolicy`; yardımcılar: `rate_from_bps`, `commission_fee`, `CommissionResolver` / `CommissionQuote`.

### 4.1 Komisyon yönetimi

- Live, Dry ve Backtest’te işlem **notional** veya borsa kuralına uygun taban üzerinden **komisyon kesintisi** uygulanır; komisyonsuz varsayım yapılmaz (Bölüm 4).

### 4.2 Dinamik komisyon oranları

- Borsanın komisyon oranlarını dönen bir **API uç noktası** mevcut ise, oranlar periyodik veya işlem öncesi çekilerek hesaplamaya dahil edilir (`CommissionQuote` kaynağı: `CommissionSource::ExchangeApi`).
- **Binance (F5):** JWT + `exchange_accounts` ile `GET /api/v1/market/binance/commission-account?symbol=BTCUSDT&segment=spot|futures` — Spot: imzalı `sapi/v1/asset/tradeFee`; USDT-M: `fapi/v1/commissionRate`. Yanıt: `maker_rate` / `taker_rate` (kesir), `source`.

### 4.3 Manuel komisyon tanımı

- API’den oran alınamadığında veya **VIP seviyesi / senaryo simülasyonu** için `CommissionPolicy::ManualBps` veya `ExchangeApiWithFallback` ile **parametrik** maker/taker bps tanımlanır.

### 4.4 Sanal bakiye (Dry ve Backtest)

- Bu modlarda nakit ve pozisyon, **gerçek hesaptan izole** sanal cüzdan üzerinden yönetilir. Başlangıç parametresi: `VirtualLedgerParams::initial_quote_balance` (genişletilebilir: çoklu varlık, marj).

---

## 5. Range trading — sinyal verisi, DB ve motor hattı

### 5.1 Mevcut durum (as-is)

- `qtss-worker`, `engine_symbols` hedefleri için `market_bars` üzerinden mum çeker.
- `trading_range` ve `signal_dashboard` sonuçları `analysis_snapshots` tablosuna yazılır (`engine_kind`, `payload` JSONB).
- **`engine_symbols.signal_direction_mode`:** `both` | `long_only` | `short_only` | `auto_segment`. Varsayılan `auto_segment`: **spot** segmentinde efektif politika **tek yönlü (long-only)** — ham model `SHORT` iken `durum` `NOTR` ve kısa sweep giriş planı temizlenir; **futures** (`futures` / `usdt_futures` / `fapi`) segmentinde **çift yönlü** (`both`). Worker payload’a `durum_model_raw`, `signal_direction_mode`, `signal_direction_effective` ekler. API: `PATCH /api/v1/analysis/engine/symbols/{id}` ile `{ "signal_direction_mode": "..." }` ve/veya `{ "enabled": true }`; web Motor listesinde seçim kutusu.
- **`range_signal_events` (F1):** Politika **sonrası** `signal_dashboard.durum` (LONG/SHORT/NOTR) önceki **geçerli** snapshot’a göre değişince `long_entry` / `long_exit` / `short_entry` / `short_exit` satırları eklenir; önceki snapshot yoksa veya `insufficient_bars` ise ilk gözlenen LONG/SHORT için sırasıyla `long_entry` / `short_entry` yazılır (yalnız NOTR ise olay yok). API `GET /api/v1/analysis/engine/range-signals`; web Motor sekmesinde liste. Sweep Telegram bildirimi: long-only iken kısa sweep, short-only iken uzun sweep **bildirilmez** (bilgi gürültüsünü azaltır).
- Web: Motor sekmesi, eşleşen sembol/interval için **snapshot özeti**, **sinyal paneli** tablosu; grafikte DB **range çizgileri** ve **sweep** işaretleri.

### 5.2 Hedef veri modeli (to-be) — olay ve pozisyon

Aşağıdaki kavramlar, grafikte **long / long exit / short / short exit** ve **açık pozisyon** göstermek için DB’de veya türetilmiş API’de tutulmalıdır:

| Kavram | Açıklama | Önerilen kalıcılık |
|--------|-----------|---------------------|
| **Sinyal olayı** | Zaman damgası, yön, olay tipi (`long_entry`, `long_exit`, `short_entry`, `short_exit`), referans fiyat, kaynak (`range_engine`, `strategy_id`) | Tablo: `strategy_signal_events` veya `analysis_snapshots` ile ilişkili ayrı `signal_events` |
| **Açık pozisyon** | Giriş zamanı/fiyatı, yön, miktar, mod (`live`/`dry`), bağlantılı run veya hesap | `positions_open` veya borsa mutabakatından türetilmiş görünüm |
| **Geçmiş işlem** | Kapanış, PnL, komisyon | Mevcut `exchange_orders` + simülasyon tabloları |

Payload şeması (ör. JSON Schema veya Rust `serde` tipleri) sürümlenmeli (`schema_version` alanı ile uyumlu: mevcut `signal_dashboard` deseni).

### 5.3 Toplama mantığı

- **Motor tick’i** veya **strateji çalışması**, sinyal durumu değiştiğinde olay üretir; idempotent upsert veya olay günlüğü (append-only) tercih edilir.
- **Dry/Live** ayrımı her kayıtta `execution_mode` veya `account_kind` ile etiketlenir.

---

## 6. Mum grafiği — işaretler (GUI)

### 6.1 Giriş / çıkış işaretleri

- **Long entry / Long exit / Short entry / Short exit** noktaları, ilgili barın **zaman ekseninde** `SeriesMarker` ile gösterilir (Lightweight Charts, `patternLabelMarkers` birleşimi).
- **Uygulama (F2):** `web/src/lib/rangeSignalMarkers.ts` — `range_signal_events` + aktif grafik mumlarıyla zaman eşlemesi; Motor’da **“DB range sinyal olayları”** checkbox’ı ile aç/kapa.
- İşaretler Bölüm 5.2’deki olay kaynağından beslenir; grafikte o bar yoksa marker düşmez.

### 6.2 Açık pozisyon

- Açık pozisyon varlığı: çizgi (giriş seviyesi), etiket veya şerit; kapanışta kaldırılır veya “kapalı” stiline geçirilir.
- Live’da kaynak: mutabakat + borsa pozisyonu; Dry’da: sanal defter tablosu.
- **Uygulama (F2 — motor türevi):** `web/src/lib/rangeOpenPositionLayer.ts` — `range_signal_events` olaylarını kronolojik işleyerek son **açık** yön ve giriş `reference_price` değerini türetir; grafik genişliğinde kesikli yatay `zigzag` katmanı (`range_position_long` / `range_position_short`). Motor’da **“DB’den türetilen açık pozisyon giriş çizgisi”** ile aç/kapa. Bu, gerçek borsa pozisyonu değildir; yalnızca DB olay zincirinin görsel özeti.

### 6.3 Mevcut range gösterimi

- DB `trading_range` payload: üst / alt / orta bant ve sweep okları — Bölüm 5.1 ile uyumludur; Bölüm 6.1 ile **çakışmadan** üst üste bindirilebilir (katman sırası ve renk konvansiyonu UI rehberinde sabitlenir).

---

## 7. Dashboard (GUI)

### 7.1 Mevcut

- Motor çekmecesinde: **Snapshot özeti**, **Sinyal paneli** — satır etiketleri Türkçe; değerler mümkünse iç içe **`signal_dashboard_v2`** (`schema_version` 3), aksi halde v1 (`durum`, trend, range metrikleri, ATR vb.). v2 varsa tablonun altında **Wire (EN)** adlı açılır blokta ham İngilizce alanlar (`<details>`; ayarlar araması: `wire`).
- **F4:** **Range / Paper özeti** kartı — üst çubuk sembol-TF, motor olay zincirinden türetilen açık yön, son 5 grafik-eşlemeli range olayı, `paper_balances` + son paper dolumlar (`GET /api/v1/orders/dry/balance`, `.../fills`). Ayarlar araması: `paper`, `dry`, `f4`, `ozet`, `islem` vb.
- **F5 (GUI):** aynı kart içinde **komisyon özeti** — motor yenilemede `GET …/market/binance/commission-defaults` (maker/taker bps, kaynak); düğmeyle `…/commission-account` (hesap kesir oranları, `exchange_accounts` gerekir). Arama: `komisyon`, `commission`, `f5`, `maker`, `taker` vb.
- **Bağlam (F7):** çekmece sekmesi — `market-context/latest` / `summary` TA satırları ve özet tabloda `ta_durum` / `ta_piyasa_modu`, mümkünse **`signal_dashboard_v2`** (`status`, `market_mode`); `engine/confluence/latest` yoksa (404) diğer istekler yine çalışır (`client.ts`). Harici worker tanımları: `GET …/external-fetch/sources` (salt okuma; yazma ops). Confluence özet metninde `data_sources_considered` → `sources …` (`formatConfluenceExtras`).

### 7.2 Hedef

- Genişletilmiş filtreleme: tenant, strateji, tarih aralığı (backtest run seçimi).

---

## 8. İzlenebilirlik ve güvenlik

- Kritik sinyal ve emir olayları audit veya structured log ile izlenir (`QTSS_AUDIT_HTTP` politikası ile uyumlu).
- Live modda **insan onayı** (`OrderIntent::requires_human_approval`) politika ile zorunlu kılınabilir.
- **RBAC:** JWT `roles` claim (`admin` / `trader` / `analyst` / `viewer`); API katmanında `require_admin`, `require_ops_roles`, `require_dashboard_roles`. Web GUI `GET /api/v1/me` ile oturum özetini alır; `app_config` yazımı ve tam config listesi yalnızca **admin**; `market_bars` backfill, `engine_symbols` POST/PATCH ve benzeri yazma uçları **trader veya admin**; grafik / snapshot okuma **viewer+**.

---

## 9. Uygulama fazları (öneri)

| Faz | İçerik | Bağımlılık |
|-----|--------|------------|
| **F0** | Bu şartname + `ExecutionMode` / `CommissionPolicy` kod hizası | Tamamlandı (domain + execution crate dokümantasyonu) |
| **F1** | `range_signal_events` tablosu + worker (`durum` kenarı) + `GET /api/v1/analysis/engine/range-signals` + web listesi | **Uygulandı** (`migrations/0016_range_signal_events.sql`) |
| **F2** | Web: giriş/çıkış marker’ları + DB olay zincirinden türetilen açık pozisyon giriş çizgisi (`rangeSignalMarkers.ts`, `rangeOpenPositionLayer.ts`, Motor checkbox’lar) | **Uygulandı** |
| **F3** | Dry sanal defter + DB (`paper_balances`, `paper_fills`) + API uçları | **Uygulandı** (`0017_paper_ledger.sql`, `apply_place`, `orders_dry.rs`) |
| **F4** | Web: Motor sekmesinde Range / Paper birleşik özet + dry API okuma | **Uygulandı** (F3 paper uçları + `App.tsx` kartı) |
| **F5** | Binance hesap komisyonu: imzalı REST + ayrıştırma + API uç | **Uygulandı** (`commission-account`, `qtss-binance` `sapi`/`fapi`) |
| **F6** | USDT-M kaldıraç API + `OrderIntent.futures` (`position_side`, `reduce_only`) + `BinanceLiveGateway` FAPI bağlama | **Uygulandı** (`POST .../futures/leverage`, `FuturesExecutionExtras`) |
| **F7** | Piyasa bağlamı + confluence: `data_snapshots`, worker `confluence`; API `market-context/latest`, **`market-context/summary`**; **Bağlam** + Motor; Nansen setup (`QTSS_NOTIFY_SETUP_*`); **dry `paper_fills` bildirimi** (`QTSS_NOTIFY_PAPER_POSITION_*`); borsa pozisyonu; **`signal_dashboard_v2`** (worker + Motor tablo değerleri) — [`PLAN_CONFLUENCE_AND_MARKET_DATA.md`](./PLAN_CONFLUENCE_AND_MARKET_DATA.md) | **Kısmen** (dry dolum MVP; borsa pozisyonu; tablo etiketleri TR, wire v2) |

**F0–F7 özeti:** F0–F6 domain, range, grafik, paper, komisyon ve futures yürütme ile tutarlı. **F7** kısmen uygulanmıştır (confluence, `data_snapshots`, `market-context/latest` + **`market-context/summary`**, **Bağlam** + Motor panelleri, isteğe bağlı **Nansen setup** özeti). **Pozisyon bildirimi (MVP):** worker `paper_fills` taraması — `QTSS_NOTIFY_PAPER_POSITION_*` / `QTSS_NOTIFY_POSITION_*` + `QTSS_NOTIFY_POSITION_TICK_SECS` (`crates/qtss-worker/src/paper_fill_notify.rs`). **Gerçek borsa açık pozisyon** bildirimi (`exchange_orders` / reconcile) açık faz. **`signal_dashboard_v2`:** worker çift yazım + Motor sinyal tablosunda değer önceliği (`signalDashboardPayload.ts`) + isteğe bağlı **Wire (EN)** açılır panel; tam İngilizce satır etiketleri ayrı opsiyon.

---

## 10. İlgili dosyalar (referans)

- `crates/qtss-domain/src/execution.rs` — mod tanımları ve açıklamalar  
- `crates/qtss-domain/src/commission.rs` — komisyon politikası ve hesap  
- `crates/qtss-binance/src/commission.rs`, `spot.rs` (`sapi_asset_trade_fee`), `futures.rs` (`fapi_commission_rate`) — F5  
- `crates/qtss-api/src/routes/market_binance.rs` — `GET .../commission-account`, `POST .../futures/leverage`  
- `crates/qtss-domain/src/orders.rs` — `FuturesExecutionExtras`, `OrderIntent.futures`  
- `crates/qtss-execution/src/binance_live.rs` — FAPI `positionSide` / `reduceOnly`  
- `crates/qtss-execution/src/lib.rs`, `crates/qtss-execution/src/dry.rs` — gateway + sanal dolum  
- `crates/qtss-storage/src/paper.rs` — paper defter repository  
- `crates/qtss-api/src/routes/orders_dry.rs` — dry REST uçları  
- `migrations/0017_paper_ledger.sql` — `paper_balances`, `paper_fills`  
- `crates/qtss-worker/src/engine_analysis.rs` — range snapshot üretimi  
- `crates/qtss-worker/src/paper_fill_notify.rs` — dry `paper_fills` bildirim döngüsü (F7 MVP)  
- `migrations/0015_engine_analysis.sql` — `engine_symbols`, `analysis_snapshots`  
- `migrations/0018_engine_signal_direction_mode.sql` — `engine_symbols.signal_direction_mode`  
- `crates/qtss-chart-patterns/src/dashboard_v1.rs` — `SignalDirectionPolicy`, `durum_model_raw` (v1 Türkçe alanlar)  
- `crates/qtss-chart-patterns/src/dashboard_v2_envelope.rs` — `signal_dashboard_v2` İngilizce çift yazım (`schema_version` 3)  
- `docs/PLAN_CONFLUENCE_AND_MARKET_DATA.md` — F7: market data, confluence, naming, Telegram  
- `docs/DATA_SOURCES_AND_SOURCE_KEYS.md` — `data_snapshots` / `external_data_sources` `source_key` adlandırma (Phase A)  
- `crates/qtss-worker/src/confluence.rs` — confluence snapshot; `signal_dashboard_v2` ile rejim ve teknik sütun (`QTSS_CONFLUENCE_ENGINE`)  
- `docs/NANSEN_TOKEN_SCREENER.md` — Nansen `POST …/token-screener`, `nansen_snapshots` / setup, API ve env rehberi  
- `web/src/App.tsx`, `web/src/api/client.ts` (`fetchPaperBalance`, `fetchPaperFills`, `fetchBinanceCommissionDefaults`, `fetchBinanceCommissionAccount`, `fetchMarketContextSummary`, `fetchMarketContextLatest`, `fetchConfluenceSnapshotsLatest`, `fetchDataSnapshots`, `fetchExternalFetchSources`, `fetchNansenSnapshot`, `fetchNansenSetupsLatest`), `web/src/components/TvChartPane.tsx`, `web/src/lib/tradingRangeDbOverlay.ts`, `web/src/lib/rangeSignalMarkers.ts`, `web/src/lib/rangeOpenPositionLayer.ts`, `web/src/lib/patternDrawingBatchOverlay.ts`, `web/src/lib/signalDashboardPayload.ts`  

---

*Belge sürümü: depo ile birlikte evrilir; faz tamamlandıkça Bölüm 5–7 “as-is” kısımları güncellenmelidir.*
