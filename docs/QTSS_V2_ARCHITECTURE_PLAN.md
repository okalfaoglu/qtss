# QTSS v2 — Mimari Yeniden Yapılandırma ve Proje Planı

> Status: Draft v1 · 2026-04-08
> Hedef: Çok piyasalı (Coin / BIST / NASDAQ), olay tabanlı, modüler, simülasyon yetenekli **analiz + otomatik yürütme + risk/portföy yönetimi** platformu.

---

## 1. Bağlam (Context)

Mevcut QTSS monorepo'su 21 Rust crate + React frontend içeriyor. Elliott dalga sadece web'de, harmonic patterns yok, formasyon/sinyal/confluence akışı dağınık, modüller DB polling + doğrudan fonksiyon çağrısı ile haberleşiyor, mimari Binance/coin'e gömülü. Hedef: aynı çekirdek motorla coin + BIST + NASDAQ analiz edebilen, modülleri event bus üzerinden konuşan, pattern → doğrulama → hedef → simülasyon zincirini eksiksiz sunan v2.

---

## 2. Tasarım İlkeleri

1. **Tek bir merkezi pivot kaynağı** — tüm pattern motorları aynı zigzag/pivot ağacını tüketir (Elliott W2 = Harmonic B = Formation shoulder).
2. **Asset-class agnostic çekirdek** — `Bar`, `Pivot`, `Pattern`, `Target` tipleri venue'den bağımsız. Borsa-özel davranış adapter crate'lerine kapsüllenir.
3. **Detector saftır** — sadece "gördüm" der; doğrulama/hedef/simülasyon ayrı katmanlardır.
4. **Event bus omurga** — modüller doğrudan birbirini çağırmaz; topic'lere publish/subscribe olur.
5. **Opsiyonel enrichment** — Nansen, on-chain, order flow, tick data çekirdek olmadan da çalışmasına izin verir (eklenince skor artırır, eklenmezse sistem bozulmaz).
6. **Her crate izole test edilebilir** — sadece `qtss-domain` tipleri + saf fonksiyonlar.
7. **Backward compat zorunlu değil** — v2 yeni şema, eski tablolar deprecate edilir.
8. **Analiz ile yürütme ayrık ama aynı dili konuşur** — analiz katmanı `TradeIntent` üretir, yürütme katmanı bunu venue-özel emirlere çevirir. Analiz motorları broker bilmez; broker adapter'ları pattern bilmez.
9. **Risk her zaman zorunlu kapı** — hiçbir emir `qtss-risk` onayı olmadan venue'ye gitmez. Kill-switch global ve venue-bazlı.
10. **Portföy venue-bazlı izole, global olarak konsolide** — her venue kendi pozisyon/bakiye doğruluğunu tutar; üst katman cross-venue net exposure görür.
11. **Config-driven** — hiçbir parametre koda gömülmez. Eşik, çarpan, timeframe, risk limiti, endpoint, retry, timeout — hepsi `qtss_config` tablosundan tipli olarak okunur, web GUI'den canlı değiştirilebilir, audit log'a yazılır. Detay: §7C.
12. **if/else minimizasyonu** — dağınık conditional yasak; trait + dispatch + lookup + guard kullan. Detay: `CLAUDE.md`.
13. **Üç çalışma modu sabit** — `live` / `dry` / `backtest`. Detay: §7D.

---

## 3. Hedef Mimari

```
┌──────────────────────────────────────────────────────────────────┐
│ Layer 8 — Web GUI (React + lightweight-charts)                  │
│   Pattern Explorer · Scenario Tree · Regime HUD · MC Fan Chart  │
│   Portfolio Dashboard · Risk HUD · Order Blotter · Kill Switch  │
└────────────────────────▲─────────────────────────────────────────┘
                         │ REST + WebSocket (live envelope push)
┌────────────────────────┴─────────────────────────────────────────┐
│ Layer 7 — qtss-api (Axum)                                        │
└────────────────────────▲─────────────────────────────────────────┘
                         │
┌────────────────────────┴─────────────────────────────────────────┐
│ Layer 6 — Decision · Risk · Execution · Portfolio                │
│   qtss-strategy      (SignalEnvelope → TradeIntent, DSL kuralları)│
│   qtss-risk          (pre-trade checks, sizing, kill-switch)     │
│   qtss-portfolio     (positions, P&L, exposure, per venue)       │
│   qtss-execution     (router; venue-agnostic OrderRequest)       │
│     ├── exec-binance        (spot + futures + margin)            │
│     ├── exec-bist           (T+2, açığa satış kuralları)         │
│     ├── exec-polygon/alpaca (US equities, PDT, options)          │
│   qtss-reconcile     (broker fills ↔ local state senkron)        │
└────────────────────────▲─────────────────────────────────────────┘
                         │ TradeIntent ← SignalEnvelope
                         │
┌────────────────────────┴─────────────────────────────────────────┐
│ Layer 5 — Prediction & Simulation                                │
│   qtss-simulator (Monte Carlo, GARCH, regime Markov)             │
│   qtss-scenario-engine (branching probability tree)              │
│   qtss-ai (LLM strateji özeti — opsiyonel)                       │
└────────────────────────▲─────────────────────────────────────────┘
                         │ ScenarioTree, ForecastDistribution
┌────────────────────────┴─────────────────────────────────────────┐
│ Layer 4 — Validation, Targets, Confluence                        │
│   qtss-validator        (tarihsel başarı, kalite skoru)          │
│   qtss-target-engine    (Fib + measured move + harmonic + S/R)   │
│   qtss-confluence       (regime-weighted aggregation)            │
│   qtss-mtf-consensus    (multi-timeframe çakıştırma)             │
└────────────────────────▲─────────────────────────────────────────┘
                         │ PatternEvent + ValidatedPattern
┌────────────────────────┴─────────────────────────────────────────┐
│ Layer 3 — Pattern Detectors (her biri Detector trait'ini impl)   │
│   qtss-elliott · qtss-harmonic · qtss-classical                  │
│   qtss-wyckoff · qtss-range (FVG, OB, liquidity)                 │
└────────────────────────▲─────────────────────────────────────────┘
                         │ Tüketim: PivotTree + Indicators + Regime
┌────────────────────────┴─────────────────────────────────────────┐
│ Layer 2 — Foundation                                             │
│   qtss-pivots      ★ MERKEZİ ZIGZAG / PIVOT TREE (L0..L3)       │
│   qtss-indicators  (EMA, MACD, BB, ATR, RSI, CVD, Fib, ADX)     │
│   qtss-regime      (ADX + BB squeeze + ATR + choppiness + HMM)  │
└────────────────────────▲─────────────────────────────────────────┘
                         │ BarClosed event
┌────────────────────────┴─────────────────────────────────────────┐
│ Layer 1 — Market Data (asset-class agnostic)                    │
│   qtss-marketdata (trait: MarketDataSource, Instrument, Calendar)│
│     ├── qtss-source-binance     (coin)                          │
│     ├── qtss-source-bist        (BIST/Matriks/Foreks adapter)   │
│     ├── qtss-source-polygon     (NASDAQ/NYSE)                   │
│     └── qtss-source-nansen      (on-chain enrichment, opsiyonel)│
└────────────────────────▲─────────────────────────────────────────┘
                         │
                ◆ Event Bus (tokio::broadcast + PG LISTEN/NOTIFY) ◆
```

### 3.1 Event topic'leri

| Topic | Publisher | Subscriber |
|---|---|---|
| `bar.closed` | source-* | indicators, pivots, regime |
| `bar.live`   | source-* | range (FVG fill), order flow |
| `pivot.updated` | qtss-pivots | tüm detector'lar |
| `regime.changed` | qtss-regime | confluence, simulator |
| `pattern.detected` | detector'lar | validator, target-engine |
| `pattern.validated` | validator | confluence, scenario-engine |
| `pattern.invalidated` | herhangi | scenario-engine, gui |
| `target.computed` | target-engine | confluence, gui |
| `scenario.updated` | scenario-engine | api → gui ws |
| `forecast.updated` | simulator | api → gui ws |
| `onchain.signal` | source-nansen | confluence (opsiyonel) |
| `signal.envelope` | confluence/scenario | strategy |
| `intent.created` | strategy | risk |
| `intent.approved` / `intent.rejected` | risk | execution / gui |
| `order.submitted` / `order.filled` / `order.canceled` | execution | portfolio, gui, reconcile |
| `position.opened` / `position.updated` / `position.closed` | portfolio | risk, gui, scenario |
| `risk.breach` | risk | execution (block), gui, notify |
| `killswitch.tripped` | risk | execution (flatten/halt), gui |

Bus implementasyonu: **in-process tokio::broadcast** (ana yol) + **PG LISTEN/NOTIFY** (worker restart dayanıklılığı, multi-process). NATS ileri faza ertelendi.

---

## 4. Merkezi Pivot Motoru (qtss-pivots) — Detay

Bu, tüm mimarinin omurgasıdır.

### 4.1 Pivot ağaç seviyeleri

| Level | Eşik kaynağı | Tüketici |
|---|---|---|
| **L0** | tick / 1m + mikro ATR(5) | order flow, sweep, kırılım teyidi |
| **L1** | 0.5 × ATR(14) | harmonic kısa, klasik küçük formasyon |
| **L2** | 1.5 × ATR(14) | klasik orta formasyon, Elliott alt dalga |
| **L3** | 4.0 × ATR(50) | Elliott ana dalga, Wyckoff faz, makro S/R |

- Her seviye bir alttakinin **alt kümesi** — tutarlılık garanti.
- Çıktı immutable `PivotTree { levels: [Vec<Pivot>; 4] }`.
- Pivot: `{ index, time, price, kind: High|Low, level, prominence, volume_at_pivot }`.
- Bir bar kapanınca tek seferde tüm seviyeler güncellenir, `pivot.updated` event'i basılır.
- Detector'lar level seçer: `ElliottDetector::required_level() = L3`.

### 4.2 Niye merkezi şart?
Şu an her motor kendi pivot ürettiği için Elliott'un W2'si Harmonic'in B noktasıyla aynı bar'a denk gelmiyor → confluence patlıyor. Merkezi olunca aynı bar = aynı pivot = aynı dil.

---

## 5. Detector Trait Sözleşmesi

```rust
pub trait PatternDetector: Send + Sync {
    fn name(&self) -> &'static str;
    fn required_pivot_level(&self) -> PivotLevel;
    fn supported_asset_classes(&self) -> &[AssetClass];

    fn detect(
        &self,
        ctx: &DetectionContext,   // bars, pivots, indicators, regime
    ) -> Vec<Detection>;
}

pub struct Detection {
    pub id: Uuid,
    pub instrument: Instrument,
    pub timeframe: Timeframe,
    pub kind: PatternKind,
    pub state: PatternState,           // Forming | Confirmed | Invalidated | Completed
    pub anchors: Vec<PivotRef>,        // çizim için
    pub structural_score: f32,         // detector'ın kendi ön skoru (kural uyumu)
    pub invalidation_price: Decimal,
    pub regime_at_detection: RegimeSnapshot,
    pub raw_meta: serde_json::Value,   // detector-specific
}
```

`confidence` ve `targets` alanlarını **detector doldurmaz** — validator ve target-engine doldurur. Bu, sorumluluğu net ayırır.

---

## 6. Çok Piyasa Soyutlaması

### 6.1 Instrument modeli
```rust
pub struct Instrument {
    pub venue: Venue,                  // Binance, Bist, Nasdaq, Nyse...
    pub asset_class: AssetClass,       // Crypto, Equity, Future, Forex
    pub symbol: String,                // BTCUSDT, THYAO, AAPL
    pub quote_ccy: Currency,
    pub tick_size: Decimal,
    pub lot_size: Decimal,
    pub session: SessionCalendar,      // 24/7, BIST 10:00-18:00 TR, NYSE 09:30-16:00 ET
    pub corporate_actions: Option<CorporateActionStream>, // split/temettü
}
```

### 6.2 Source adapter trait
```rust
pub trait MarketDataSource {
    fn list_instruments(&self) -> Vec<Instrument>;
    async fn subscribe_bars(&self, inst: &Instrument, tf: Timeframe) -> BarStream;
    async fn historical(&self, inst: &Instrument, tf: Timeframe, range: TimeRange) -> Vec<Bar>;
    fn capabilities(&self) -> SourceCapabilities;  // tick? l2? funding?
}
```

### 6.3 Adapter'lar

| Crate | Piyasa | Notlar |
|---|---|---|
| `qtss-source-binance` | Crypto | mevcut WS + REST, tick + l2 + funding mevcut |
| `qtss-source-bist` | BIST | Foreks/Matriks/İş Yatırım API; seans + temettü + bedelsiz |
| `qtss-source-polygon` | NASDAQ/NYSE | Polygon.io; pre/post market, splits, dividends |
| `qtss-source-nansen` | On-chain enrichment | sadece coin; `onchain.signal` event'i basar, çekirdek bağımlı değil |
| `qtss-source-tradingview` | (opsiyonel ileri) | TradingView relay |

**Kritik:** `qtss-elliott`, `qtss-harmonic`, `qtss-classical` venue'yi bilmez. Sadece `Bar` + `PivotTree` görür. Bu sayede THYAO 1D'de Elliott aramak BTCUSDT 4H ile aynı kod yolunu kullanır.

---

## 7. Simülasyon & Forecast Detayı (qtss-simulator)

### 7.1 Yöntem
1. **Tarihsel kalibrasyon**: log-getiri dağılımı, otokorelasyon, GARCH(1,1) volatilite
2. **Rejim Markov zinciri**: `qtss-regime` çıktısından geçiş matrisi
3. **Pattern-aware injection**: aktif `Confirmed` / `Forming` pattern'lerin hedef ve invalidasyonları senaryolara prior olarak enjekte edilir
4. **Monte Carlo**: 10k–50k yol, her bar için fiyat dağılımı
5. **Çıktı**:
   - `ForecastDistribution { time_axis, percentiles[5,25,50,75,95], regime_probabilities }`
   - `ScenarioTree { nodes: [{ trigger, probability, target_band, children[] }] }`

### 7.2 GUI
- **Fan chart**: medyan + p25/p75 + p5/p95 bantları, gelecek 30/90/365/2000 bar
- **Scenario tree paneli**: bull/bear/neutral dalları, her düğümde tetikleyici şart ve olasılık
- **Pattern overlay**: forming pattern'lerin tamamlanma olasılığı bar üstünde

> Not: Çok uzun horizonlarda (2030) güven aralığı çok geniş çıkar — tek nokta tahmin yerine **olasılık dağılımı + en olası rejim sekansı** sunulur.

---

## 7B. Execution · Risk · Portfolio Katmanı (Detay)

Analiz katmanının sonu `SignalEnvelope`. Buradan sonra **karar → risk → emir → pozisyon → mutabakat** zinciri başlar. Bu zincir analiz motorlarından tamamen ayrıdır ama aynı `qtss-domain` tiplerini konuşur.

### 7B.1 Akış

```
SignalEnvelope
   │  qtss-strategy (DSL kuralları, regime filter, MTF teyit)
   ▼
TradeIntent { instrument, side, sizing_hint, entry_zone, sl, tps[],
              time_in_force, source_signals[], conviction }
   │  qtss-risk (pre-trade checks)
   ▼
ApprovedIntent  ──or──  RejectedIntent { reason }
   │
   ▼  qtss-execution (venue router)
OrderRequest (venue-agnostic) → broker adapter → venue API
   │
   ▼  fills stream
order.filled → qtss-portfolio updates → position events
   │
   ▼  qtss-reconcile (periyodik broker truth karşılaştırma)
```

### 7B.2 qtss-strategy

- **DSL**: YAML/JSON ile kural tanımı.
  ```yaml
  name: "trend-follow-elliott-w3"
  applies_to: { asset_classes: [crypto, equity], venues: [binance, polygon] }
  when:
    - regime.trend in [strong_up]
    - elliott.wave == 3
    - mtf.consensus >= 0.7
    - confluence.score >= 0.65
  intent:
    side: long
    sizing: risk_pct(0.5)        # %0.5 hesap riski
    entry: limit_at(zone.low)
    sl: invalidation_price
    tps: [fib(1.618), fib(2.618), measured_move]
    time_stop: 24h
  ```
- Aynı sembol için birden fazla strateji aktif olabilir; portföy katmanı çakışmaları çözer (netting).
- Strategy `signal.envelope` dinler, `intent.created` yayar.

### 7B.3 qtss-risk — pre-trade checks

Her `TradeIntent` aşağıdaki kapılardan geçer (sıralı, ilk red kazanır):

1. **Kill-switch durumu** — global veya venue-bazlı halt aktifse red
2. **Account state** — margin yetersiz, equity altında, drawdown limiti aşıldı?
3. **Pozisyon limitleri** — max pozisyon sayısı (toplam / venue / sembol / asset class)
4. **Exposure limitleri** — max gross/net exposure (USD bazında, cross-venue)
5. **Korelasyon limiti** — açık pozisyonlarla yüksek korelasyonlu yeni giriş engeli (BTC + ETH + SOL aynı yön bloklanır)
6. **Sektör/grup limiti** — BIST'te bankacılık %30 üstü engeli, NASDAQ'ta tek sektör konsantrasyon
7. **Venue kuralları** — BIST: T+2 nakit, açığa satış yasak listesi, devre kesici saatleri; NASDAQ: PDT (<25k için günlük 3 daytrade), pre/post likidite; Binance: leverage cap, isolated/cross
8. **Sizing hesaplama** — risk_pct × stop_distance → adet, sonra venue tick/lot size'a yuvarlama
9. **Slippage / spread sanity** — anlık spread > eşik ise red
10. **Notional minimum** — venue'nin min order büyüklüğü

**Sizing modelleri** (strateji seçer):
- `risk_pct(x)`: hesap equity × x / |entry - sl|
- `kelly(fraction)`: validator'ın hit rate + R/R'sinden Kelly fraksiyonu
- `fixed_notional(x)`: sabit USD/TRY notional
- `vol_target(x)`: ATR-bazlı, hedef portföy volatilitesi

**Kill-switch tetikleyicileri**:
- Günlük max drawdown (%/USD)
- Ardışık N losing trade
- API hata oranı eşiği
- Manuel (GUI/Telegram)
- Venue-bazlı ayrı kill-switch (Binance kapanınca BIST çalışmaya devam eder)

### 7B.4 qtss-portfolio

- **Per-venue izole hesap durumu**: bakiye, açık pozisyonlar, açık emirler, gerçekleşmiş/gerçekleşmemiş P&L, fee, funding (futures)
- **Cross-venue konsolidasyon**: tüm venue'ler tek bir USD/TRY denominator'da → toplam equity, gross/net exposure, asset class dağılımı, sektör dağılımı, korelasyon matrisi
- **Pozisyon lifecycle**: opened → scaled (TP1 hit, kısmi çıkış) → trailing → closed
- **Trailing/Break-even/Partial TP** mantığı portföy katmanında, strateji-agnostik
- **P&L attribution**: hangi sinyal/strateji hangi P&L'i üretti (validator'ın self-learning'ine geri besleme)
- **Vergi/raporlama**: BIST için TR vergi raporu, NASDAQ için 1099 benzeri (ileri faz)

### 7B.5 qtss-execution

- **OrderRequest**: venue-agnostik `{ instrument, side, type: Market|Limit|Stop|StopLimit|OCO|Iceberg, qty, price?, tif, reduce_only?, post_only?, client_order_id }`
- **Broker adapter trait**:
  ```rust
  pub trait BrokerAdapter {
      async fn place(&self, req: OrderRequest) -> Result<OrderAck>;
      async fn cancel(&self, id: &OrderId) -> Result<()>;
      async fn modify(&self, id: &OrderId, ...) -> Result<()>;
      async fn account_snapshot(&self) -> Result<AccountState>;
      fn supports(&self) -> ExecutionCapabilities; // OCO? bracket? short? margin?
      fn calendar(&self) -> &SessionCalendar;
  }
  ```
- **Akıllı router**: adapter capability'sine göre OCO/bracket emrini ya tek emirle ya da bacak bacak gönderir
- **Idempotency**: `client_order_id` ile retry-safe
- **Order book aware execution** (ileri): büyük order'ı VWAP/TWAP/iceberg ile parçalama

**Adapter listesi**:
| Crate | Venue | Notlar |
|---|---|---|
| `qtss-exec-binance` | Binance Spot/Futures/Margin | mevcut, refactor |
| `qtss-exec-bist` | BIST (broker API) | İş Yatırım/Garanti/Matriks; T+2, devre kesici |
| `qtss-exec-alpaca` | NASDAQ/NYSE | başlangıç için Alpaca, sonra IBKR |
| `qtss-exec-paper` | Tüm venue'ler | dry-run / paper trading (Faz 1'de zorunlu) |

### 7B.6 qtss-reconcile

- Periyodik (örn 30sn) broker'dan account snapshot çek → local `qtss-portfolio` ile karşılaştır
- Drift varsa: `reconcile.drift` event'i + auto-correct (broker truth kazanır)
- Eksik fill, missed cancel, manual broker müdahalesi → log + notify
- Restart sonrası state rebuild

### 7B.7 Venue-özel davranış örnekleri

| Konu | Binance | BIST | NASDAQ |
|---|---|---|---|
| Session | 7/24 | 10:00–18:00 TR + açılış/kapanış seansları | 09:30–16:00 ET + pre/post |
| Settlement | anlık | T+2 | T+2 |
| Açığa satış | serbest (futures) | yasaklı sembol listesi var | locate gerekli |
| Min order | 5 USDT | 1 lot, fiyat adımı kademeli | 1 share |
| Devre kesici | yok | %10 üst/alt halt | LULD bantları |
| Temettü/split | yok | corporate action stream | corporate action stream |
| Funding | futures | yok | yok |
| Tick verisi | bedava WS | broker'a göre | Polygon paid |

Tüm bu kurallar **risk + execution adapter'ında** kapsüllenir; analiz katmanı bilmez.

## 7C. Config Registry (qtss-config) — Sıfırdan Tasarım

**Mevcut durum**: Repo'da iki paralel tablo (`app_config` ve `system_config`) var. Her ikisi de scope, kategori, validation, default, audit, tip bilgisi ve versioning'den yoksun. V2'de bu iki tablo deprecate edilir ve yerine **tek normalize edilmiş hiyerarşi** gelir.

### 7C.1 Tasarım Hedefleri
1. **Tek doğru kaynak** (single source of truth) — iki paralel tablo yok
2. **Scope hiyerarşisi**: instrument > strategy > venue > asset_class > global
3. **Tip güvenliği** — JSON Schema validation
4. **Audit** — her değişiklik immutable kaydedilir, rollback mümkün
5. **Hot-reload** — değişiklik PG NOTIFY ile in-process cache'e yansır
6. **Secret separation** — sırlar ayrı tabloda (`secrets`), config'te key referansı
7. **Optimistic lock** — concurrent edit çakışması yakalanır
8. **Self-describing** — UI bu tablodan formu otomatik üretir

### 7C.2 Şema
```sql
-- Şema kataloğu: hangi key var, tipi, validasyonu, default'u
CREATE TABLE config_schema (
    key             TEXT PRIMARY KEY,             -- "risk.max_drawdown_pct"
    category        TEXT NOT NULL,                -- "risk" | "strategy" | "indicators" | ...
    subcategory     TEXT,                         -- "limits" | "sizing" | ...
    value_type      TEXT NOT NULL,                -- "int" | "float" | "string" | "bool" | "enum" | "object" | "array" | "duration" | "decimal"
    json_schema     JSONB NOT NULL,               -- JSON Schema (min/max/regex/enum/required)
    default_value   JSONB NOT NULL,
    unit            TEXT,                         -- "%" | "USD" | "bars" | "seconds"
    description     TEXT NOT NULL,                -- UI tooltip
    ui_widget       TEXT,                         -- "slider" | "input" | "select" | "json_editor"
    requires_restart BOOLEAN NOT NULL DEFAULT false,
    is_secret_ref   BOOLEAN NOT NULL DEFAULT false, -- value bir secret_id ise
    sensitivity     TEXT NOT NULL DEFAULT 'normal', -- "low" | "normal" | "high" (high = onay zorunlu)
    deprecated_at   TIMESTAMPTZ,
    introduced_in   TEXT,                         -- "v2.0"
    tags            TEXT[]
);

-- Scope tipleri (enum yerine tablo — esneklik)
CREATE TABLE config_scope (
    id          BIGSERIAL PRIMARY KEY,
    scope_type  TEXT NOT NULL,            -- "global" | "asset_class" | "venue" | "strategy" | "instrument" | "user"
    scope_key   TEXT NOT NULL,            -- "binance" | "BTCUSDT" | "trend_v1" | "" (global)
    parent_id   BIGINT REFERENCES config_scope(id),  -- hiyerarşi
    UNIQUE (scope_type, scope_key)
);

-- Asıl değerler (override'lar)
CREATE TABLE config_value (
    id           BIGSERIAL PRIMARY KEY,
    key          TEXT NOT NULL REFERENCES config_schema(key),
    scope_id     BIGINT NOT NULL REFERENCES config_scope(id),
    value        JSONB NOT NULL,
    version      INT NOT NULL DEFAULT 1,        -- optimistic lock
    enabled      BOOLEAN NOT NULL DEFAULT true,
    valid_from   TIMESTAMPTZ,                   -- gelecek tarihli aktivasyon
    valid_until  TIMESTAMPTZ,                   -- otomatik expire
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by   UUID REFERENCES users(id),
    UNIQUE (key, scope_id)
);

-- Immutable audit log
CREATE TABLE config_audit (
    id           BIGSERIAL PRIMARY KEY,
    key          TEXT NOT NULL,
    scope_id     BIGINT,
    action       TEXT NOT NULL,             -- "create" | "update" | "delete" | "rollback"
    old_value    JSONB,
    new_value    JSONB,
    changed_by   UUID,
    changed_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    reason       TEXT NOT NULL,             -- zorunlu
    correlation  UUID,                      -- toplu değişiklikleri grupla
    hash_prev    BYTEA,                     -- hash chain (audit immutability)
    hash_self    BYTEA
);

CREATE INDEX idx_config_value_key ON config_value(key);
CREATE INDEX idx_config_value_scope ON config_value(scope_id);
CREATE INDEX idx_config_audit_key ON config_audit(key, changed_at);
```

### 7C.3 Çözümleme (resolution) algoritması
Bir key okunduğunda scope hiyerarşisi yukarıdan aşağıya taranır, ilk eşleşme döner:

```
instrument:BTCUSDT → strategy:trend_v1 → venue:binance →
asset_class:crypto → global → schema.default_value
```

Her crate tipli enum + helper kullanır:
```rust
// qtss-config crate
pub enum ConfigKey { /* derive macro ile */ }

impl ConfigStore {
    fn get<T: DeserializeOwned>(&self, key: ConfigKey, ctx: &ResolveCtx) -> T;
    fn set<T: Serialize>(&self, key: ConfigKey, scope: Scope, val: T,
                         by: UserId, reason: &str) -> Result<()>;
    fn watch(&self, key: ConfigKey) -> ConfigWatcher;  // tokio stream
}
```

`ResolveCtx { instrument, strategy, venue, asset_class, user }` — değerin hangi bağlamda istendiğini söyler.

### 7C.4 Hot-reload mekanizması
1. UI'dan `config.set` çağrılır
2. Trigger → `pg_notify('config_changed', json{key, scope_id})`
3. Tüm worker'larda dinleyen `qtss-config` cache'i ilgili entry'yi invalidate eder
4. Bir sonraki `get()` taze değer döner
5. `watch()` kullanan modüller event alır, gerekirse yeniden başlatma yapar (`requires_restart=true` keys için manuel restart prompt)

### 7C.5 GUI — Config Editor
- **Sol panel**: Kategori → subcategory → key ağacı (`config_schema`'dan üretilir)
- **Orta panel**: Seçili key için form (widget tipi `ui_widget`'tan)
- **Sağ panel**: Aktif scope override'ları (hangi venue/strategy farklı değer kullanıyor)
- **Üst bar**: Scope seçici (global / venue / strategy / instrument)
- **Diff preview** + **reason zorunlu** kaydetmeden önce
- **Audit history** her key için tab
- **Rollback** tek tıkla eski versiyona dönüş (yeni audit entry oluşturur)
- **Bulk import/export** JSON
- **Validation** client + server (json_schema)
- **High sensitivity** keys için 4-eyes (ikinci kullanıcı onayı)

### 7C.6 Mevcut Yapıdan Migration

Bu Faz 7'de yapılacak ayrı bir task. Plan §11A'da detaylı.

---

### 7C.2 Erişim
```rust
// qtss-config crate
pub trait ConfigStore {
    fn get<T: DeserializeOwned>(&self, key: &str, scope: Scope) -> Result<T>;
    fn set<T: Serialize>(&self, key: &str, value: T, scope: Scope, by: &str) -> Result<()>;
    fn watch(&self, key: &str) -> ConfigWatcher; // change notification
}
```
- Hot-reload: değişiklik olduğunda PG NOTIFY → in-process cache invalidasyonu
- Tipli: her crate kendi `ConfigKey` enum'unu tanımlar, derleme-zamanı garanti
- Scope çözümleme sırası: `instrument > strategy > venue > global`

### 7C.3 GUI
**Config Editor** sayfası:
- Kategori bazlı ağaç (risk, strategy, indicators, execution, ui, ai)
- Inline edit + validation
- Diff preview + reason zorunlu
- Audit history her key için
- Bulk import/export (JSON)
- Rollback

### 7C.4 Migration ile seed
Tüm default değerler migration dosyasında. Yeni bir parametre eklemek = migration + GUI'de görünür olur.

---

## 7D. Çalışma Modları (live / dry / backtest)

Üç sabit mod, runtime context olarak taşınır. Feature flag değil — her worker/strategy başlangıçta hangi modda çalıştığını bilir.

| Mod | Veri | Emir | Yazıldığı tablo |
|---|---|---|---|
| **live** | Gerçek (canlı WS) | Gerçek borsaya gönderilir | `orders`, `fills`, `positions` |
| **dry** | Gerçek (canlı WS) | Borsaya **gitmez**, lokal simüle edilir | aynı tablolar, `mode='dry'` kolonu |
| **backtest** | Tarihsel (`bars` tarihsel range) | Lokal simüle | `bt_orders`, `bt_fills`, `bt_positions` (ayrı schema) |

- **dry** modu yeni stratejiyi canlı veriyle ama parasız test etmek için (eski "paper trading"). Promote prosedürü: backtest yeşil → dry'da N gün gözlem → live.
- **backtest** harness `qtss-validator` ve strateji geliştirme için ayrı bir worker (`qtss-backtest`). Aynı detector/risk/execution kodunu kullanır, sadece zaman kaynağı simüle.
- Mod, `accounts` tablosunda venue başına saklanır. Aynı venue'de aynı anda hem live hem dry hesabı olabilir (farklı stratejiler).
- "Shadow mode" kavramı kaldırıldı — gereksiz karmaşa.

---

## 7E. Komisyon, Vergi, Raporlama

### Komisyon (qtss-fees)
- Venue + asset_class + ürün bazlı komisyon modeli (`qtss_config`'te tablolanmış)
  - Binance spot: maker/taker bps, BNB indirim
  - Binance futures: maker/taker bps, funding payment
  - BIST: BSMV, kurtaj, yüzdelik + min ücret
  - NASDAQ: SEC fee, FINRA TAF, broker komisyonu (Alpaca: 0; IBKR: tier)
- `qtss-portfolio` her fill'de komisyonu hesaplar ve P&L'den düşer
- Backtest için aynı komisyon tablosu kullanılır (gerçekçi sonuç)

### Vergi (qtss-tax)
- TR: BSMV, gelir vergisi (BIST temettü stopajı, kripto henüz belirsiz)
- US: kısa/uzun vadeli capital gains, wash sale rule
- Her venue + jurisdiction için pluggable vergi modülü
- Yıl sonu raporu üretir

### Raporlama (qtss-reporting)
- **Daily**: equity, P&L, açık pozisyonlar, risk kullanımı, slippage özeti
- **Monthly**: strateji bazlı performans, sharpe/sortino, max DD, hit rate
- **Yearly**: vergi raporu (jurisdiction bazlı PDF/CSV), best execution raporu
- **On-demand**: backtest raporu, MiFID/SEC compliance export
- Çıktı: PDF, CSV, JSON; e-posta veya dosya sistemine

---

## 7F. Enterprise Katmanı (qtss-enterprise umbrella)

Aşağıdaki başlıklar enterprise pro seviye için zorunlu, modüler olarak eklenir:

| Başlık | Crate | Açıklama |
|---|---|---|
| **Audit log** | `qtss-audit` | Immutable, append-only; kim ne zaman ne yaptı (config, emir, kill-switch, login). Hash chain ile tamper-evident. |
| **RBAC + Multi-user** | `qtss-auth` | Roller: admin, trader, analyst, viewer. Per-venue izinler. JWT + refresh token. |
| **Secret management** | `qtss-secrets` | API key/secret pgcrypto ile şifreli; ileri faz Vault entegrasyonu. Plain text yasak. |
| **Observability** | `qtss-metrics` | Prometheus exporter; latency, fill rate, slippage, hata oranı, event lag. Grafana dashboard preset. |
| **Health checks** | `qtss-health` | `/healthz`, `/readyz`; venue connectivity, DB, event bus, broker API. K8s probe uyumlu. |
| **Rate limiter** | `qtss-ratelimit` | Venue API ban yememek için token bucket per-venue. |
| **Circuit breaker** | (qtss-execution içi) | Broker hata oranı eşiği aşılınca o venue otomatik halt. |
| **Feature flags** | `qtss_config` üzerinden | Stratejiyi/venue'yi/AI'yı anlık aç-kapa, canary rollout. |
| **Webhook out** | `qtss-webhook` | Event'leri dış sistemlere push (Slack, custom HTTP). |
| **DR / Backup** | infra | PG streaming replication + günlük snapshot; restore drill prosedürü. |
| **A/B test** | `qtss-experiment` | Strateji versiyonlarını paralel koştur, performans karşılaştır. |
| **Replay / Time travel** | `events_log` üstünde | Geçmiş bir anı reprodüce et; bug repro + post-mortem. |
| **Compliance** | `qtss-reporting` içinde | Trade reporting, best execution kayıtları, MiFID/SEC export. |

---

## 7H. Scheduled Jobs (qtss-scheduler)

Bazı veriler sürekli akmaz; zamanlanmış olarak çekilir. **Nansen** en belirgin örnek (smart money holdings, token god mode, cex flow saatlik veya günlük). Başka adaylar: corporate actions, BIST KAP duyuruları, NASDAQ earnings calendar, funding rate snapshot, halving sayacı, vergi kuru güncellemeleri.

### 7H.1 Tasarım

Yeni crate: **`qtss-scheduler`**. Cron + interval tabanlı job runner. Job tanımları **config-driven** (yine `config_schema` üzerinden), kod değişikliği gerektirmez.

```sql
CREATE TABLE scheduled_jobs (
    id            UUID PRIMARY KEY,
    name          TEXT UNIQUE NOT NULL,             -- "nansen.smart_money_holdings"
    handler       TEXT NOT NULL,                    -- "qtss-source-nansen::pull_smart_money"
    schedule      TEXT NOT NULL,                    -- cron veya "every:1h" veya "at:09:00,17:00"
    timezone      TEXT NOT NULL DEFAULT 'UTC',
    enabled       BOOLEAN NOT NULL DEFAULT true,
    args          JSONB,                            -- handler-specific
    timeout_secs  INT NOT NULL DEFAULT 300,
    max_retries   INT NOT NULL DEFAULT 3,
    backoff       JSONB,                            -- {"type":"exponential","base_ms":1000}
    overlap_policy TEXT NOT NULL DEFAULT 'skip',    -- "skip" | "queue" | "kill_previous"
    last_run_at   TIMESTAMPTZ,
    next_run_at   TIMESTAMPTZ,
    last_status   TEXT,                             -- "success" | "failed" | "running"
    created_at    TIMESTAMPTZ DEFAULT now()
);

CREATE TABLE job_runs (
    id           BIGSERIAL PRIMARY KEY,
    job_id       UUID REFERENCES scheduled_jobs(id),
    started_at   TIMESTAMPTZ NOT NULL,
    finished_at  TIMESTAMPTZ,
    status       TEXT NOT NULL,                     -- "running" | "success" | "failed" | "timeout"
    attempt      INT NOT NULL DEFAULT 1,
    output       JSONB,                             -- handler dönüşü, hata mesajı, vb.
    error        TEXT,
    rows_affected INT
);
```

### 7H.2 Handler trait
```rust
pub trait ScheduledHandler: Send + Sync {
    fn name(&self) -> &'static str;
    async fn run(&self, ctx: JobContext) -> Result<JobOutput>;
}

// Handler'lar registry'ye kaydolur (linkme veya ctor crate ile derleme zamanı)
inventory::submit!(NansenSmartMoneyHandler);
```

Yeni job eklemek = bir handler yaz + DB'ye satır ekle (UI'dan). Kod değişmez, deploy yok.

### 7H.3 Nansen için spesifik joblar

| Job | Schedule | Veri | Hedef tablo |
|---|---|---|---|
| `nansen.smart_money_holdings` | `every:1h` | Smart money cüzdan holdings | `nansen_holdings` |
| `nansen.token_god_mode` | `every:30m` | Token-bazlı flow özet | `nansen_token_flow` |
| `nansen.cex_flow` | `every:15m` | Borsa giriş/çıkış akışları | `nansen_cex_flow` |
| `nansen.dex_trades_top` | `every:5m` | Top DEX trade'ler | `nansen_dex_trades` |
| `nansen.alerts_sync` | `at:00:00 UTC` | Alert kuralları sync | `nansen_alerts` |
| `nansen.daily_snapshot` | `at:00:05 UTC` | Günlük snapshot | `nansen_daily` |
| `nansen.fullsync_on_startup` | `on:startup` | Sistem açıldığında tam çek | tüm yukarıdaki tablolar |

`on:startup` → sistem açılışında bir kez koşar (uzun kapalı kalmadan sonra eksik verileri doldurur).

### 7H.4 Diğer scheduled job adayları
- `bist.kap_announcements` — BIST KAP duyuruları (her 5dk seans içinde)
- `nasdaq.earnings_calendar` — daily 06:00 ET
- `corporate_actions.sync` — daily, tüm venue'ler
- `binance.funding_snapshot` — her 8 saatte funding payment'ten önce
- `fx.usd_try` — TR vergi raporu için kur, daily TCMB
- `validator.retrain` — pattern başarı oranı yeniden hesaplama, weekly
- `reporting.daily_pnl` — daily 23:55 venue local time
- `reconcile.broker_state` — every 30s (interval, cron değil)

### 7H.5 Çalışma garanti modeli
- **Distributed lock**: aynı job'u birden fazla worker çalıştırmasın (Postgres advisory lock)
- **Skip vs Queue**: önceki run hala koşuyorsa `overlap_policy`'ye göre davran
- **Catch-up**: sistem kapalıyken kaçırılan run'lar için `args.catch_up=true` ile bir sonraki run kapsamı genişletilir
- **Manuel tetik**: GUI'den "Run Now" butonu (audit'e kaydedilir)
- **Metric**: her run latency + success rate Prometheus'a

### 7H.6 GUI — Scheduler paneli
- Job listesi + sonraki çalışma + son durum
- Enable/disable toggle
- Schedule editor (cron builder)
- Run history (son 100)
- Manuel "Run Now"
- Hata loglarına drill-down

---

## 7G. qtss-ai Yeniden Konumlandırma

**Mevcut durum**: `qtss-ai` LLM ile karar üretiyor (strategic/tactical/operational), `ai_decisions` tablosuna yazıyor, **manuel onay** bekliyor. Otomatik live/dry açma-kapatma yapmıyor.

**V2'de**: `qtss-ai` bir **Strategy Provider** olur. DSL stratejilerinin yanında "strategy = LLM call" da tanımlanabilir.

```yaml
name: "ai-tactical-claude-v1"
type: ai
provider: anthropic
model: claude-opus-4-6
context:
  - last_50_bars
  - active_patterns
  - regime_snapshot
  - portfolio_state
prompt_template: "tactical_v3"
auto_approve:
  live: false       # live'da insan onayı zorunlu (default)
  dry: true         # dry'da otomatik
  backtest: true
output_schema: trade_intent_v1
```

- Çıktı: yine `TradeIntent` → aynı `qtss-risk` pipeline → aynı `qtss-execution`
- LLM kararı da risk kapılarından geçer (asla bypass yok)
- `auto_approve` config-driven; live'da default `false` (insan onayı), dry'da `true`
- Onay UI'sı: GUI'de "AI Decisions" sayfası, bekleyen kararlar, tek tık onay/red
- Telegram/Discord onay (mevcut `notify_telegram_config` korunur)
- Mevcut 3 katman (strategic/tactical/operational) DSL'de farklı strategy entries olarak modellenebilir

---

## 8. Veri Şeması (yeni tablolar)

| Tablo | Amaç |
|---|---|
| `instruments` | Çok piyasa enstrüman katalogu |
| `bars` | venue + symbol + tf normalize OHLCV (mevcut `market_bars` rename) |
| `pivots` | PivotTree snapshot'ları (level başına) |
| `pattern_detections` | Tüm detector çıktıları (lifecycle: forming→confirmed→…) |
| `pattern_validations` | Validator skor + tarihsel hit rate snapshot'ı |
| `targets` | Hedef cluster'lar (TP1/TP2/TP3, SL, method breakdown) |
| `regime_snapshots` | Bar bazında rejim kaydı |
| `scenarios` | ScenarioTree JSON snapshot'ları |
| `forecasts` | MC ForecastDistribution snapshot'ları |
| `events_log` | Event bus replay için |
| `accounts` | Venue başına hesap (api key ref, mode: paper/shadow/live) |
| `trade_intents` | Strategy çıktısı, risk kararı, audit trail |
| `orders` | Tüm gönderilen emirler (lifecycle: submitted/ack/partial/filled/canceled/rejected) |
| `fills` | Bireysel doluşlar (fee, liquidity, venue trade id) |
| `positions` | Açık + kapalı pozisyon geçmişi, P&L attribution |
| `risk_events` | Pre-trade red, breach, kill-switch olayları |
| `reconcile_log` | Drift tespit + düzeltme kayıtları |
| `portfolio_snapshots` | Periyodik equity/exposure snapshot (raporlama) |
| `config_schema` | Key kataloğu (tip, validation, default, açıklama) |
| `config_scope` | Scope hiyerarşisi (global/asset_class/venue/strategy/instrument) |
| `config_value` | Aktif override değerler (versioned, optimistic lock) |
| `config_audit` | Immutable config değişiklik log'u (hash chain) |
| `scheduled_jobs` | Cron/interval job tanımları |
| `job_runs` | Scheduled job çalışma geçmişi |
| `audit_log` | Immutable kim-ne-zaman-ne (hash chain) |
| `users`, `roles`, `user_roles` | RBAC |
| `secrets` | Şifreli API key/secret saklama |
| `fees` | Venue + ürün bazlı komisyon tablosu |
| `tax_lots` | FIFO/LIFO vergi lot takibi |
| `reports` | Üretilmiş rapor metadata + dosya path |
| `bt_orders`, `bt_fills`, `bt_positions` | Backtest izole tabloları |
| `ai_decisions` | (mevcut) AI strateji çıktıları, onay durumu |

Mevcut `analysis_snapshots`, `signal_dashboard`, `trading_range` tabloları **deprecate** — okunur ama yazılmaz, v2 stabilleşince düşülür.

---

## 9. Crate Listesi (yeni / değişen / silinecek)

### Yeni
- `qtss-pivots` ★
- `qtss-regime`
- `qtss-elliott` (Rust, sıfırdan)
- `qtss-harmonic`
- `qtss-classical` (eski `qtss-chart-patterns` yerine, sıfırdan)
- `qtss-wyckoff`
- `qtss-range` (FVG, order block, liquidity)
- `qtss-validator`
- `qtss-target-engine`
- `qtss-mtf-consensus`
- `qtss-simulator`
- `qtss-scenario-engine`
- `qtss-eventbus`
- `qtss-marketdata` (trait + Instrument)
- `qtss-source-bist`
- `qtss-source-polygon`
- `qtss-risk` ★
- `qtss-portfolio` ★
- `qtss-reconcile`
- `qtss-exec-bist`
- `qtss-exec-alpaca` (NASDAQ — başlangıç)
- `qtss-exec-ibkr` (NASDAQ — enterprise, ileri faz)
- `qtss-config` ★ (tipli config registry, hot-reload)
- `qtss-fees`
- `qtss-tax`
- `qtss-reporting`
- `qtss-audit`
- `qtss-auth` (RBAC)
- `qtss-secrets`
- `qtss-metrics` (Prometheus)
- `qtss-health`
- `qtss-ratelimit`
- `qtss-webhook`
- `qtss-experiment` (A/B test)
- `qtss-backtest` (dedicated backtest worker)
- `qtss-scheduler` ★ (cron/interval job runner, distributed lock, catch-up)

### Değişecek
- `qtss-domain` — yeni ortak tipler
- `qtss-api` — WS push, yeni endpoint'ler, RBAC entegrasyonu
- `qtss-storage` — yeni şema
- `qtss-confluence` — küçülür, sadece aggregation
- `qtss-source-binance` — eski `qtss-binance`'tan refactor
- `qtss-ai` — Strategy Provider olarak yeniden konumlandırılır (§7G); 3 katman DSL entry'leri olur, çıktısı `TradeIntent`, mevcut `approval` mekanizması korunur ama `auto_approve` config-driven hale gelir

### Deprecate
- `qtss-chart-patterns` (qtss-classical + qtss-pivots devralır)
- `qtss-tbm` (mantığı validator + pattern detection'a dağılır)
- `qtss-analysis::engine_loop` (worker yeniden yazılır, event-driven)

---

## 10. Yol Haritası (Faz Bazlı)

> Süre tahmini vermiyorum — fazlar bitince bir sonraki başlar.

### Faz 0 — Onay & İskele
- Bu plan dosyasının onayı
- `CLAUDE.md` kodlama kuralları aktif (if/else min, config-driven, katman ayrımı)
- Yeni `qtss-domain` tipleri (`Instrument`, `Bar`, `Pivot`, `PivotTree`, `Detection`, `RegimeSnapshot`, `Target`, `ScenarioTree`, `ForecastDistribution`, `TradeIntent`, `OrderRequest`, `RunMode`)
- `qtss-config` (tipli registry, hot-reload, audit) — **diğer her crate'in ön koşulu**
- `qtss-eventbus` (tokio broadcast + PG NOTIFY köprüsü)
- `qtss-audit` + `qtss-secrets` + `qtss-auth` iskeletleri
- Yeni şema migration'ları (yeni tablolar, eski tablolar dokunulmaz)
- Default config seed'leri

### Faz 1 — Foundation
- `qtss-pivots` (L0–L3 çok katmanlı zigzag, testler altın veriye karşı)
- `qtss-regime` (ADX + BB squeeze + ATR + choppiness + HMM rejim)
- `qtss-marketdata` trait + mevcut Binance kodunun `qtss-source-binance` olarak refactor'u
- Worker'ın event-driven iskeleti (bar.closed → indicators → pivots → regime)

### Faz 2 — Detector'lar
- `qtss-elliott` sıfırdan (impulse 1-5, corrective ABC, diagonal, MTF hierarchy, kural kataloğu test'lerle)
- `qtss-harmonic` (Gartley, Butterfly, Bat, Crab, Shark, Cypher, Deep Crab + PRZ)
- `qtss-classical` (H&S, double/triple, flag, wedge, rectangle, cup&handle, pennant)
- `qtss-wyckoff` (PS, SC, AR, ST, Spring, Test, SOS, LPS, BU)
- `qtss-range` (FVG, order block, liquidity pool, equal highs/lows)

### Faz 3 — Validation, Target, Confluence
- `qtss-validator` (tarihsel hit rate hesabı, regime-bazlı başarı, self-learning hook'u)
- `qtss-target-engine` (Fib + measured move + harmonic PRZ + Elliott projection + S/R snap + cluster)
- `qtss-mtf-consensus` (15m/1h/4h/1d çakıştırma)
- `qtss-confluence` küçültme (sadece final aggregation)

### Faz 4 — Forecast & Simülasyon
- `qtss-simulator` (GARCH + regime Markov + pattern-aware MC)
- `qtss-scenario-engine` (branching tree, çelişen pattern reconciliation)
- API WS endpoint'leri (`/ws/scenarios`, `/ws/forecasts`)

### Faz 4B — Execution, Risk, Portfolio, Fees, AI
- `qtss-strategy` DSL parser + evaluator (`signal.envelope` → `intent.created`)
- `qtss-risk` pre-trade kapıları + sizing modelleri + kill-switch
- `qtss-portfolio` per-venue + cross-venue konsolidasyon, P&L attribution
- `qtss-execution` router + venue-agnostik OrderRequest
- `qtss-fees` komisyon hesaplama (config-driven)
- `qtss-exec-binance` refactor (mevcut emir kodunu adapter trait'ine taşı)
- `qtss-reconcile` periyodik broker truth karşılaştırma
- `qtss-ai` Strategy Provider olarak refactor — DSL'e bağlanır, `auto_approve` config flag (live=false default)
- `live` / `dry` / `backtest` runtime mode framework'ü
- `qtss-backtest` worker (tarihsel veri replay)

### Faz 5 — GUI v2 (sıfırdan yeni shell — detay §9A)
- Pattern Explorer paneli
- Scenario Tree görselleştirici
- Regime HUD
- Monte Carlo fan chart
- **Portfolio Dashboard** (per-venue + konsolide equity, exposure, P&L)
- **Risk HUD** (kullanılan risk, açık pozisyonlar, drawdown, kill-switch durumu)
- **Order Blotter** (canlı emirler, fills, audit trail)
- **Strategy Manager** (DSL editör, live/dry/backtest toggle)
- **Config Editor** (kategori ağacı, inline edit, diff, audit history, rollback)
- **AI Decisions** (bekleyen kararlar, onay/red, prompt+output görüntüleme)
- **Audit Log Viewer**
- **User & Roles** (RBAC yönetimi)
- Mevcut Elliott TS motoru sadece **çizim adaptörü** olarak kalır (mantık backend'de)

### Faz 6A — NASDAQ (öncelik 2)
- `qtss-source-polygon` (NASDAQ market data: bars, splits, dividends, pre/post)
- `qtss-exec-alpaca` (commission-free, paper hesap dahili, hızlı go-live)
- US session calendar, PDT rule, LULD bantları
- US fees + SEC/FINRA fee tablosu

### Faz 6B — BIST (öncelik 3)
- `qtss-source-bist` (Foreks veya Matriks adapter)
- `qtss-exec-bist` (broker API — İş Yatırım/Garanti/Matriks Trader, lisans süreci)
- BIST seans takvimi, devre kesici, açığa satış yasak listesi, T+2
- BSMV + kurtaj fee modeli + TR vergi raporu

### Faz 6C — Enterprise NASDAQ (opsiyonel)
- `qtss-exec-ibkr` (IBKR Web API veya TWS Gateway, global piyasa erişimi, opsiyon, futures)

### Faz 6D — Enterprise & Compliance
- `qtss-audit` hash chain, immutable log
- `qtss-auth` RBAC tam entegrasyon (admin/trader/analyst/viewer)
- `qtss-secrets` pgcrypto + ileride Vault
- `qtss-metrics` Prometheus + Grafana dashboard preset
- `qtss-health` K8s probe
- `qtss-ratelimit` venue başına token bucket
- `qtss-webhook` dış sistem push
- `qtss-experiment` A/B test framework
- `qtss-tax` (TR + US jurisdiction)
- `qtss-reporting` (daily/monthly/yearly + compliance export)

### Faz 7 — Stabilizasyon
- Eski tabloların düşürülmesi
- `qtss-chart-patterns`, `qtss-tbm`, eski `qtss-binance` silinmesi
- 10 yıllık çoklu piyasa backtest harness
- Performans tuning + load test
- DR drill

---

## 9A. Web GUI v2 — Detaylı Tasarım

Eski React app'e dokunulmaz; v2 **`/v2` route'u altında ayrı bir uygulama** olarak çalışır. Tamamlanınca cutover yapılır.

### 9A.1 Teknoloji Yığını
| Katman | Seçim | Gerekçe |
|---|---|---|
| Framework | **React 18 + TypeScript 5** | mevcut ekip bilgisi |
| Build | **Vite 6** | hızlı HMR, mevcut araç |
| Routing | **React Router v6 (data router)** | nested routes, loader/action |
| State (server) | **TanStack Query v5** | cache, refetch, optimistic |
| State (client) | **Zustand** | hafif, boilerplate yok |
| Forms | **React Hook Form + Zod** | tipli validation |
| UI library | **shadcn/ui (Radix + Tailwind)** | erişilebilir, özelleştirilebilir, ağır değil |
| Tailwind | **v4** | utility-first, dark mode hazır |
| Icons | **Lucide** | shadcn ile uyumlu |
| Tablolar | **TanStack Table v8** | sort/filter/pagination/virtual |
| Grafik (candlestick) | **lightweight-charts v5** | TradingView, mevcut bilgi |
| Grafik (analitik) | **Recharts** veya **Visx** | dashboard kart grafikleri için |
| Layout dock | **react-grid-layout** | sürükle-bırak dashboard |
| Real-time | **Native WebSocket + reconnect wrapper** | API WS push |
| i18n | **i18next** | TR + EN |
| Theme | **CSS variables + dark/light** | sistem tercihi takip |
| Test | **Vitest + Testing Library + Playwright** | unit + e2e |

**Mimari prensipler:**
- **Modüler** — her sayfa bağımsız bir feature module (`/features/portfolio/`, `/features/strategies/` …). Routes, components, hooks, API client her feature'a aittir.
- **Görsel-ağırlıklı** — sayı tablolarından önce grafik. Her metrik için en az bir görselleştirme.
- **Responsive-first** — mobile (>=375px), tablet (>=768px), desktop (>=1280px), 4K (>=1920px). Tailwind breakpoint'leri.
- **Touch-friendly** — chart pinch-zoom, swipe nav, tap target ≥ 44px.
- **Offline-tolerant** — TanStack Query cache, son değerleri göster, "stale" rozeti.
- **Erişilebilir** — Radix tabanlı, klavye nav, ARIA, kontrast WCAG AA.
- **Code-split** — her route lazy load.

### 9A.2 Layout & Navigasyon

**Hamburger menü kullanılmaz.** Çoklu sayfa, kalıcı navigasyon, hızlı geçiş:

```
┌─────────────────────────────────────────────────────────┐
│  Top Bar: Logo · Venue Switcher · Mode Badge · 🔔 · 👤  │
├──────┬──────────────────────────────────────────────────┤
│      │                                                  │
│ Side │           Main Content (route outlet)            │
│ Nav  │                                                  │
│      │                                                  │
│ ☰    │                                                  │
│      │                                                  │
└──────┴──────────────────────────────────────────────────┘
```

- **Sol Sidebar**: Kalıcı, ikon + label. Daraltılabilir (sadece ikon mode). Mobilde drawer olarak açılır.
- **Top bar**: Aktif venue (Binance/BIST/NASDAQ) seçici, mod rozeti (live/dry/backtest, renk kodlu), notification bell, user avatar/menu.
- **Breadcrumb**: Her sayfada üstte yol gösterici.
- **Command palette** (Cmd+K): hızlı sembol arama, sayfa atlama, action tetikleme.

### 9A.3 Sayfalar (Routes)

Her madde ayrı bir route + ayrı bir feature module:

| # | Route | Sayfa | İçerik özeti |
|---|---|---|---|
| 1 | `/v2/dashboard` | **Dashboard** | Konsolide P&L, anlık/günlük/haftalık/aylık/yıllık equity grafikleri, açık pozisyon özeti, risk kullanımı, son sinyaller, top kazanan/kaybeden, son emirler |
| 2 | `/v2/portfolio` | **Portfolio** | Per-venue + konsolide bakiye, açık pozisyonlar, açık emirler, allocation pie, exposure barlar, korelasyon ısı haritası |
| 3 | `/v2/portfolio/positions/:id` | **Position Detail** | Tek pozisyon: giriş, scale, fills, P&L curve, ilişkili sinyaller |
| 4 | `/v2/pnl` | **P&L Analytics** | Detaylı kar/zarar: günlük/haftalık/aylık/yıllık karşılaştırma, sharpe/sortino/calmar, max DD, hit rate, R/R, equity curve, drawdown grafiği |
| 5 | `/v2/charts/:venue/:symbol` | **Chart Workspace** | Ana grafik sayfası — candlestick + overlay'ler + drawing tools (§9A.5) |
| 6 | `/v2/scanner` | **Symbol Scanner** | Tüm sembolleri filtrele: rejim, pattern, hacim, momentum |
| 7 | `/v2/patterns` | **Pattern Explorer** | Aktif pattern'ler tablosu (Elliott/Harmonic/Klasik/Wyckoff/Range), confidence, target, invalidation, drill-down |
| 8 | `/v2/scenarios` | **Scenario Tree** | Branching senaryo görselleştirici, olasılık ağacı |
| 9 | `/v2/forecast` | **Forecast / Monte Carlo** | Fan chart, percentile bantları, rejim olasılıkları |
| 10 | `/v2/regime` | **Regime HUD** | Piyasa rejimi monitörü, geçiş sinyalleri, ADX/BB squeeze/ATR/choppiness |
| 11 | `/v2/strategies` | **Strategy Manager** | Strateji listesi, DSL editör (Monaco), live/dry/backtest toggle |
| 12 | `/v2/strategies/:id/backtest` | **Backtest Lab** | Strateji backtest çalıştırma, equity curve, trade listesi, metric'ler |
| 13 | `/v2/orders` | **Order Blotter** | Canlı emirler, fill akışı, audit trail |
| 14 | `/v2/risk` | **Risk Center** | Kullanılan risk %, açık pozisyon limit, sektör/korelasyon, kill-switch durumu, breach geçmişi |
| 15 | `/v2/ai` | **AI Decisions** | Bekleyen LLM kararları, prompt+output, onay/red, geçmiş başarı |
| 16 | `/v2/scheduler` | **Scheduled Jobs** | Job listesi, sonraki run, history, manuel tetik |
| 17 | `/v2/config` | **Config Editor** | Kategori ağacı, scope seçici, inline edit, diff/audit/rollback |
| 18 | `/v2/audit` | **Audit Log** | Kim ne zaman ne yaptı (immutable, filter, export) |
| 19 | `/v2/users` | **Users & Roles** | RBAC yönetimi |
| 20 | `/v2/reports` | **Reports** | Daily/monthly/yearly raporlar, vergi, compliance export, PDF/CSV |
| 21 | `/v2/notifications` | **Notifications** | Telegram/Discord/Webhook ayarları, log |
| 22 | `/v2/health` | **System Health** | Venue connectivity, event lag, broker latency, error rate, Grafana embed |
| 23 | `/v2/settings` | **User Settings** | Tema, dil, default venue, notification tercihleri |

**Toplam 23 sayfa** — hepsi ayrı route, kalıcı sidebar'dan tek tıkla. Hamburger içine gizlenmez.

### 9A.4 Dashboard Detayı

Sayfa **`react-grid-layout`** ile sürükle-bırak kart sistemi. Kullanıcı kartları boyutlandırır/taşır, layout user başına kaydedilir.

Varsayılan kartlar:
1. **Equity Curve Card** (büyük) — anlık + günlük/haftalık/aylık/yıllık toggle, area chart, MoM/YoY %
2. **P&L Snapshot Card** — bugün / bu hafta / bu ay / bu yıl, renkli rakamlar, sparkline
3. **Open Positions Card** — açık pozisyon sayısı, toplam unrealized, top winner/loser
4. **Risk Usage Gauge** — kullanılan risk %, max DD'ye uzaklık, gauge widget
5. **Active Patterns Card** — son 24h tespit edilen pattern sayısı, türlere göre dağılım
6. **Recent Orders Card** — son 10 emir, durum rozeti
7. **Top Movers Card** — izlenen sembollerden en çok hareket edenler, mini sparkline'lar
8. **Regime HUD Mini** — her venue için anlık rejim rozeti
9. **Notification Feed Card** — son alarm/sinyal akışı
10. **AI Decisions Pending Card** — onay bekleyen LLM kararları

Her kart:
- Refresh interval config-driven
- Tam ekran modu
- CSV export
- "Detay sayfasına git" linki

### 9A.5 Chart Workspace — Çizim Araçları

Sayfa: `/v2/charts/:venue/:symbol`. Tam ekran candlestick + zengin etkileşim.

**Çizim araçları (sol toolbar)**:
- Trend line, horizontal line, vertical line, ray
- Fibonacci retracement, extension, fan, time zones
- Channel (parallel, regression)
- Rectangle, ellipse, triangle
- Pitchfork (Andrew's)
- Elliott wave label (1-5, A-C)
- Harmonic XABCD pattern
- Text annotation, arrow, callout
- Measure tool (price/time delta)
- Brush (free draw)
- Eraser, undo/redo

**Overlay'ler (üst toolbar)**:
- İndikatörler: EMA, BB, RSI, MACD, ATR, VWAP, Volume Profile, vb.
- Backend pattern overlay'leri (Detection event'lerinden otomatik): Elliott sayımı, harmonic PRZ, klasik formasyon, FVG, order block, S/R levels
- Pivot ağacı katmanları (L0/L1/L2/L3 toggle)
- Forecast fan chart overlay
- Aktif pozisyonlar (giriş/SL/TP çizgileri)
- Active orders (emir seviyeleri)

**Etkileşim**:
- Pinch/wheel zoom, drag pan
- Hover crosshair (OHLCV + indikatör değerleri)
- Çoklu timeframe split view (1m/5m/15m/1h aynı sembol)
- Compare mode (BTC vs ETH normalized overlay)
- Drawing'ler kullanıcı + sembol + timeframe başına kaydedilir (`chart_drawings` tablosu)
- Snapshot al → PNG indir veya rapora ekle
- "Place order from chart" — SL/TP'yi sürükleyerek emir oluştur

### 9A.6 P&L Analytics Detayı

Sayfa: `/v2/pnl`. **Anlık + günlük + haftalık + aylık + yıllık** zaman aralıkları ayrı tab'larda, her birinde:

- **Equity curve** (area + drawdown overlay)
- **Period bar chart** (her gün/hafta/ay P&L barı, yeşil/kırmızı)
- **Cumulative return %**
- **Calendar heatmap** (GitHub contribution tarzı, günlük P&L)
- **Metrics tablosu**: Total P&L, Win rate, Profit factor, Sharpe, Sortino, Calmar, Max DD, Avg win/loss, Avg holding time
- **Strateji bazlı kırılım**: hangi strateji ne kadar getirdi (stacked bar)
- **Sembol bazlı kırılım**: top kazanan/kaybeden semboller
- **Venue bazlı kırılım**: Binance vs BIST vs NASDAQ
- **Asset class kırılım**: crypto vs equity
- **Trade distribution** (R-multiple histogram)
- **Excursion analysis** (MAE/MFE)
- **Filter bar**: tarih aralığı, venue, strategy, sembol, asset class, mod (live/dry)
- **Export**: PDF rapor, CSV, vergi formatlı export

### 9A.7 Real-time Push

- API tarafında `/ws/v2` endpoint'i
- Subscribe topic'ler: `bars.{venue}.{symbol}.{tf}`, `patterns`, `orders`, `positions`, `forecasts`, `notifications`, `risk`, `ai_decisions`
- Frontend'de tek `WSClient` singleton; her sayfa kendi topic'lerine subscribe olur, unmount'ta unsubscribe
- Reconnect with exponential backoff, last-event-id ile catch-up

### 9A.8 Yeni DB Tabloları (Web tarafı)
| Tablo | Amaç |
|---|---|
| `chart_drawings` | Kullanıcı çizimleri (kullanıcı + sembol + tf + JSON) |
| `dashboard_layouts` | Kullanıcı dashboard kart yerleşimi |
| `user_preferences` | Tema, dil, varsayılan venue, vb. |
| `saved_filters` | Scanner/blotter kayıtlı filtreler |

---

## 10A. Migration Task — Eski Config'ten V2 Config'e

**Ne zaman**: Faz 7 (Stabilizasyon). V2 paralel çalışırken eski tablolar dokunulmaz; geçiş sonunda toplu yapılır.

### Adımlar

1. **Envanter çıkarma**
   - `app_config` ve `system_config`'teki tüm key'leri listele
   - Her key için: kullanıldığı crate, kullanım yeri (grep), gerçek tipi, anlamı çıkarılır
   - Çıktı: `docs/CONFIG_MIGRATION_INVENTORY.md`

2. **Schema mapping**
   - Her eski key için `config_schema` satırı tanımla:
     - yeni `key` (kategorik isimlendirme: `risk.max_drawdown_pct` gibi)
     - `value_type`, `json_schema`, `default_value`, `unit`, `description`
     - `category` + `subcategory`
   - Eski → yeni eşleme tablosu: `migrations/v2_config_mapping.json`

3. **Scope tespiti**
   - Eski sistemde scope yok — her key default olarak `global` scope'a gider
   - Bazı key'ler aslında venue-bazlı (örn `binance.tick_size`) — manuel scope ataması yapılır

4. **Validation pass**
   - Eski değerler yeni `json_schema`'ya karşı validate edilir
   - Geçmeyen değerler rapor edilir, manuel düzeltme

5. **Migration script**
   - Idempotent SQL: `app_config` + `system_config` → `config_schema` + `config_value` + ilk `config_audit` entry'si (action=`migrated_from_v1`)
   - Hash chain başlatılır

6. **Code migration**
   - Eski `resolve_system_string()`, `resolve_worker_*` çağrıları `qtss-config::get<T>()` ile değiştirilir
   - Her crate'te grep ile bul-değiştir, derleme ile garanti
   - PR'lar crate başına bölünür (review kolaylığı)

7. **Cutover**
   - Eski tabloları read-only yap (trigger ile UPDATE/INSERT engelle)
   - 1 hafta gözlem (eski koda hiç düşülmediğinden emin ol)
   - `DROP TABLE app_config; DROP TABLE system_config;`

8. **GUI cutover**
   - Eski "App Config" ve "System Config" sayfaları kaldırılır
   - Yeni "Config Editor" canlı

### Risk azaltma
- V2 paralel çalıştığı süre boyunca eski tablolar **kaynak değil** — sadece referans için tutulur
- Migration script test ortamında en az 3 kez çalıştırılır
- Tek-key hatalı eşleme = production'da default'a düşer (felaket değil); audit'ten geri alınır

---

## 10B. Default Risk Limitleri (öneri — config'e seed edilir)

Tüm değerler `config_value` tablosuna global scope ile seed edilir. Web GUI'den canlı değiştirilebilir. Asset-class bazlı override'lar da seed'lenir.

### Hesap seviyesi
| Key | Default | Açıklama |
|---|---|---|
| `risk.account.max_daily_loss_pct` | **2.0%** | Günlük equity kaybı eşiği → kill-switch |
| `risk.account.max_weekly_loss_pct` | **5.0%** | Haftalık |
| `risk.account.max_drawdown_pct` | **10.0%** | Peak-to-trough → tüm trading durur |
| `risk.account.max_open_positions` | **15** | Toplam |
| `risk.account.max_open_per_venue` | **8** | Venue başına |
| `risk.account.max_open_per_symbol` | **1** | Aynı sembolde tek pozisyon |
| `risk.account.max_consecutive_losses` | **5** | Tetikleyici |

### Pozisyon seviyesi
| Key | Default | Açıklama |
|---|---|---|
| `risk.position.default_risk_pct` | **0.5%** | Tek trade risk = equity × 0.5% / stop_distance |
| `risk.position.max_risk_pct` | **1.5%** | Üst sınır (strateji geçemez) |
| `risk.position.min_rr_ratio` | **1.5** | Min R/R, altındaysa intent reddedilir |
| `risk.position.max_leverage` | **3.0x** | Genel üst sınır |

### Exposure
| Key | Default | Açıklama |
|---|---|---|
| `risk.exposure.max_gross_pct` | **300%** | Toplam pozisyon notional / equity |
| `risk.exposure.max_net_pct` | **150%** | Net long-short |
| `risk.exposure.max_per_asset_class_pct` | **70%** | Tek asset-class konsantrasyonu |
| `risk.exposure.max_per_sector_pct` | **30%** | Tek sektör |
| `risk.exposure.max_correlation_cluster` | **40%** | Yüksek korelasyonlu pozisyonlar toplamı |
| `risk.exposure.correlation_threshold` | **0.7** | Pearson, üstündekileri cluster say |

### Asset-class override önerileri (scope: asset_class:*)
| Asset class | max_leverage | default_risk_pct | max_open |
|---|---|---|---|
| **crypto spot** | 1.0x | 0.5% | 8 |
| **crypto futures** | 5.0x | 0.3% | 6 |
| **equity (BIST)** | 1.0x | 0.7% | 5 |
| **equity (NASDAQ)** | 2.0x | 0.7% | 8 |

### Slippage / spread
| Key | Default | Açıklama |
|---|---|---|
| `risk.slippage.max_bps` | **20** | Anlık spread > eşik → red |
| `risk.slippage.expected_bps_market` | **5** | Market order tahmini |
| `risk.slippage.expected_bps_limit` | **0** | Limit order |

### Kill-switch tetikleyicileri
| Key | Default | Açıklama |
|---|---|---|
| `killswitch.daily_dd_trip` | true | Günlük DD aşılınca |
| `killswitch.consecutive_losses_trip` | true | Üst üste kayıp |
| `killswitch.api_error_rate_pct` | **20%** | Son 5dk hata oranı |
| `killswitch.broker_disconnect_secs` | **120** | Broker bağlantı koparsa |
| `killswitch.action` | "halt_new_only" | "halt_new_only" \| "flatten_all" \| "halt_venue" |
| `killswitch.cooldown_minutes` | **60** | Tetikten sonra otomatik açılma süresi (manuel reset gerektirir) |

### Sizing modeli defaultları
| Key | Default | Açıklama |
|---|---|---|
| `sizing.default_model` | "risk_pct" | "risk_pct" \| "kelly" \| "fixed_notional" \| "vol_target" |
| `sizing.kelly.fraction` | **0.25** | Quarter-Kelly |
| `sizing.vol_target.annualized_pct` | **15%** | Hedef portföy volatilitesi |

> Bu değerler **muhafazakâr** seçildi. Tradingview/profesyonel literatür ortalaması civarı. GUI'den hesabın büyüklüğüne göre ayarlanır. Yeni hesap için ilk hafta default'lar; performans gözlemlendikten sonra yukarı çekilebilir.

---

## 11. Doğrulama / Test Stratejisi

- **Altın veri setleri**: bilinen Elliott sayımları, harmonic örnekleri, klasik formasyonlar el ile etiketlenmiş → her commit'te regression testi
- **Property testleri**: pivot ağacının seviye-içerik ilişkisi (L3 ⊂ L2 ⊂ L1)
- **Backtest harness**: `qtss-validator` 10 yıllık BTC + SPY + THYAO üzerinde her detector için hit rate raporu
- **Event replay**: `events_log` üzerinden geçmiş bir günü yeniden oynatma → deterministik sonuç
- **End-to-end**: GUI'de bir sembol seç → 30 saniyede pattern + target + scenario + forecast görünmeli

---

## 12. Açık Kararlar (User onayı bekleyen)

1. **BIST veri sağlayıcı** — Foreks mi, Matriks mi, İş Yatırım mı, başka? (Lisans + maliyet)
2. **NASDAQ veri sağlayıcı** — Polygon.io mu, Alpaca mı, IBKR mı?
3. **Tick verisi**: coin için Binance trade WS aktif mi, yoksa Faz 7'ye mi ertelendi?
4. **GUI yeniden yazımı**: mevcut React app üstüne mi, ayrı v2 route mu?
5. **AI/LLM katmanı (qtss-ai)**: bu fazda dahil mi, sonraya mı?
6. **Auth/multi-tenant**: v2'de değişiyor mu?
7. **Live trading skoru**: hangi venue'lerde gerçek para açacağız (Faz 4B sonu için)? Önce paper, sonra shadow, sonra live mı?
8. **BIST broker API**: hangi aracı kurum API'si (İş Yatırım, Garanti, Matriks Trader)? Lisans + onay süreci uzun olabilir.
9. **NASDAQ broker**: Alpaca yeterli mi, IBKR mı şart?
10. **Risk limitleri default değerleri**: max günlük drawdown, max pozisyon sayısı, max korelasyon — kim belirleyecek (sen mi, config UI mı)?
11. **Vergi/raporlama**: TR vergi mevzuatı (kripto + BIST) ve US vergi (NASDAQ) ayrı ayrı pluggable; başlangıçta hangisi öncelik?
12. **BIST broker seçimi** — İş Yatırım API mi, Garanti FXTrader mı, Matriks Trader mı? (Lisans + onay maliyeti farklı.)
13. **NASDAQ rollout** — Faz 6A'da Alpaca yeterli, Faz 6C'de IBKR opsiyonel önerim. Onaylıyor musun?

---

## 13. Riskler

- **Pivot tanımının pattern motorlarına dayatılması** — bazı detector'lar kendi pivot toleransını isteyebilir. Mitigation: detector seviye seçer + opsiyonel local refinement hook.
- **MC simülasyonun yanıltıcı hassasiyet vermesi** — GUI'de mutlaka güven aralığı + "tahmin değildir" disclaimer.
- **BIST/NASDAQ veri maliyeti** — Faz 6'ya kadar ertelendi, çekirdek coin ile validate edilir.
- **Migration süresi** — eski + yeni paralel çalışacak, çift-yazma kaçınılmaz.

---

## 14. Sonraki Adım

Bu dokümanın gözden geçirilmesi ve **Açık Kararlar** bölümünün yanıtlanması. Onay sonrası Faz 0 başlar.
