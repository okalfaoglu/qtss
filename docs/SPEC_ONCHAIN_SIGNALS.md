# QTSS — On-Chain Sinyal Entegrasyonu Teknik Şartnamesi

Mevcut QTSS altyapısına (Rust workspace, PostgreSQL, Axum API, qtss-worker) on-chain ve türev piyasa sinyallerinin entegrasyonu. Bu belge mimari kararları, sinyal kaynaklarını, skorlama mantığını, mevcut analiz modülleri ile birleşim (confluence) stratejisini ve uygulama adımlarını tanımlar.

Kod referansları: `qtss-worker` (collector loop'ları), `qtss-storage` (`external_data_sources` / `external_data_snapshots`), `qtss-nansen`, `qtss-notify`, `qtss-chart-patterns` (`trading_range`, `signal_dashboard_v1`, ACP kanal taraması), `web/src/lib/elliottEngineV2/`.

---

## 1. Sinyal kaynakları (final)

Tüm aktif kaynaklar maksimum 5 saniye gecikmeyle çalışır (Nansen hariç). Gecikmeli ücretsiz kaynaklar çıkarıldı, yerlerine doğrudan borsa API'leri veya Coinglass real-time verileri konuldu.

### 1.1 Aktif kaynaklar

| Sinyal | Kaynak | Gecikme | Maliyet | Tick | Endpoint |
|--------|--------|---------|---------|------|----------|
| Funding rate / OI | Binance FAPI | ~0s | Ücretsiz | 60s | `GET /fapi/v1/fundingRate`, `GET /fapi/v1/openInterest` |
| Likidasyon haritası | Coinglass | ~1-5s | Ücretsiz | 300s | `GET /public/v2/liquidation_history` |
| Long/short ratio | Binance FAPI | ~0s | Ücretsiz | 300s | `GET /futures/data/globalLongShortAccountRatio` |
| Taker buy/sell volume | Binance FAPI | ~0s | Ücretsiz | 60s | `GET /futures/data/takerlongshortRatio` |
| Perp bias / whale pos. | Hyperliquid | ~0s | Ücretsiz | 60s | `POST /info` → `metaAndAssetCtxs` |
| HL whale pozisyonları | Hyperliquid | ~0s | Ücretsiz | 120s | `POST /info` → `clearinghouseState` |
| Exchange netflow | Coinglass | ~1-5s | Ücretsiz | 300s | `GET /public/v2/exchange/netflow` |
| Exchange balance | Coinglass | ~5s | Ücretsiz | 300s | `GET /public/v2/exchange/balance` |
| Smart money screening | Nansen | ~1-10dk | Ücretli | 1800s | `POST /api/v1/token-screener` |
| TVL trend (tamamlayıcı) | DeFi Llama | ~10-30dk | Ücretsiz | 1800s | `GET /v2/protocols` |

### 1.2 Çıkarılan kaynaklar ve gerekçeleri

| Çıkarılan | Gerekçe | Yerine geçen |
|-----------|---------|--------------|
| CryptoQuant (free tier) | Ücretsiz kullanıcılar yalnızca günlük çözünürlük + 1 saat gecikme ile veri alır. Saatlik ve 10dk veri ücretli planlarda açılır. Sinyal üretmek için çok yavaş. | Binance taker buy/sell vol (0s) + Coinglass netflow (1-5s) |
| Whale Alert (free tier) | Dakikada 10 istek limiti + blok doğrulama gecikmesi (1-5dk). WebSocket yalnızca ücretli. | Hyperliquid whale positions (0s) + Coinglass exchange balance (5s) |
| DeFi Llama DEX volume | TVL verisi 10-30dk gecikir, DEX volume günlük güncellenir. Aktif sinyal için yetersiz. | Binance spot taker volume (0s). TVL trend göstergesi olarak 1800s tick ile tamamlayıcı kalır. |

---

## 2. Mimari — Veri toplama

### 2.1 Mevcut altyapı (değişiklik gerektirmez)

QTSS zaten iki veri toplama mekanizmasına sahip:

**Harici çekim motorları** (`qtss-worker/src/engines/*`, ortak `external_common.rs`): `external_data_sources` satırlarını `tick_secs` ile çeker, ham JSON’u **`data_snapshots`** tablosuna yazar (migration 0021–0024 ile `external_data_snapshots` kaldırıldı). Worker restart gerektirmez.

**`nansen_engine`** (`qtss-worker/src/nansen_engine.rs`): Nansen Token Screener API'sine özel HTTP istemcisi. Kredi takibi (`x-nansen-credits-used`, `x-nansen-credits-remaining`), yetersiz kredi durumunda uzun uyku (`NANSEN_INSUFFICIENT_CREDITS_SLEEP_SECS`), `nansen_snapshots` tablosuna yazım.

### 2.2 Birleşik provider mimarisi (yeni)

Nansen ve external_fetch ayrı loop'lar olarak kalır (Nansen'in kredi yönetimi, özel header'ları ve hata semantiği genel fetch'ten farklıdır), ancak ortak bir `DataSourceProvider` trait'i ile soyutlanır. Scorer katmanı her iki tablodan da okur.

```
trait DataSourceProvider {
    fn source_key(&self) -> &str;
    fn tick_secs(&self) -> u64;
    async fn fetch(&self) -> Result<SnapshotPayload>;
}

impl DataSourceProvider for NansenProvider { ... }      // Mevcut nansen_engine
impl DataSourceProvider for HttpGenericProvider { ... }  // Mevcut external_fetch_engine
impl DataSourceProvider for FutureProvider { ... }       // TheGraph, WebSocket vb.
```

Yeni bir API eklemek için: (1) trait implemente et, (2) worker'a register et, (3) scorer'a `score_fn` ekle. Mevcut kodu değiştirmek gerekmez.

### 2.3 Veri akışı

```
Dış API'ler
    ├── Hyperliquid (POST /info)
    ├── Binance FAPI (GET /fapi/v1/*)
    ├── Coinglass (GET /public/v2/*)
    ├── DeFi Llama (GET /v2/protocols)
    └── Nansen (POST /api/v1/token-screener)
         │
         ▼
qtss-worker
    ├── engines (binance / coinglass / hyperliquid / …) ──→ data_snapshots
    └── nansen_engine ──────────→ nansen_snapshots
         │
         ▼
SignalScorer (yeni modül)
    ├── Snapshot JSON okur (her iki tablodan)
    ├── Her source_key için score_fn dispatch
    ├── Rejime göre ağırlıklı aggregate
    └── onchain_signal_scores tablosuna yazar
         │
         ▼
Çıktılar
    ├── signal_dashboard v2 (mevcut + confluence)
    ├── qtss-notify (eşik aşımı → Telegram, Discord, e-posta)
    └── API (GET /api/v1/analysis/onchain-signals/*)
```

---

## 3. Veritabanı şeması

### 3.1 Migration: kaynak seed’leri (`0021` … `0023`)

`external_data_sources` tablosuna kaynak satırları (projede `0021_external_data_fetch.sql`, ilgili seed migration’ları). Worker restart gerektirmez.

```sql
INSERT INTO external_data_sources (key, method, url, body_json, tick_secs, description) VALUES

-- Hyperliquid: perp meta + funding + OI (tüm coinler tek istekte)
('hl_meta', 'POST', 'https://api.hyperliquid.xyz/info',
 '{"type":"metaAndAssetCtxs"}'::jsonb, 60,
 'Hyperliquid perp meta: funding, OI, mark price'),

-- Hyperliquid: whale pozisyon takibi
('hl_whale_positions', 'POST', 'https://api.hyperliquid.xyz/info',
 '{"type":"clearinghouseState","user":"0xWHALE_CÜZDAN_ADRESİ"}'::jsonb, 120,
 'Hyperliquid whale pozisyon — Whale Alert yerine'),

-- Binance FAPI: funding rate
('binance_funding_btc', 'GET',
 'https://fapi.binance.com/fapi/v1/fundingRate?symbol=BTCUSDT&limit=10',
 NULL, 60, 'Binance BTCUSDT funding rate'),

-- Binance FAPI: long/short account ratio
('binance_ls_ratio', 'GET',
 'https://fapi.binance.com/futures/data/globalLongShortAccountRatio?symbol=BTCUSDT&period=1h&limit=10',
 NULL, 300, 'Binance long/short account ratio'),

-- Binance FAPI: taker buy/sell volume ratio (CryptoQuant yerine)
('binance_taker_vol', 'GET',
 'https://fapi.binance.com/futures/data/takerlongshortRatio?symbol=BTCUSDT&period=5m&limit=10',
 NULL, 60, 'Binance BTCUSDT taker buy/sell ratio — real-time'),

-- Coinglass: exchange netflow (CryptoQuant yerine)
('coinglass_netflow', 'GET',
 'https://open-api.coinglass.com/public/v2/exchange/netflow?symbol=BTC&ex=Binance',
 NULL, 300, 'BTC Binance exchange netflow'),

-- Coinglass: aggregated OI
('coinglass_oi', 'GET',
 'https://open-api.coinglass.com/public/v2/open_interest?symbol=BTC',
 NULL, 300, 'BTC open interest all exchanges'),

-- Coinglass: likidasyon geçmişi
('coinglass_liq', 'GET',
 'https://open-api.coinglass.com/public/v2/liquidation_history?symbol=BTC&time_type=h1',
 NULL, 300, 'BTC liquidation history'),

-- Coinglass: exchange balance (Whale Alert yerine)
('coinglass_exchange_balance', 'GET',
 'https://open-api.coinglass.com/public/v2/exchange/balance?symbol=BTC',
 NULL, 300, 'BTC all-exchange balance — whale transfer proxy'),

-- DeFi Llama: TVL trend göstergesi (yavaş sinyal, tamamlayıcı)
('defillama_tvl_top', 'GET',
 'https://api.llama.fi/v2/protocols',
 NULL, 1800, 'All protocol TVL list — trend göstergesi');
```

**Not:** Coinglass API key gerekiyorsa `headers_json` alanına ekleyin:
```sql
UPDATE external_data_sources
SET headers_json = '{"coinglassSecret":"YOUR_KEY"}'::jsonb
WHERE key LIKE 'coinglass_%';
```

### 3.2 Migration: `0030_onchain_signal_scores.sql`

Skor tablosu (depoda **`migrations/0030_onchain_signal_scores.sql`**). Sonraki migrasyonlar ek kolonlar ve `app_config` anahtarları ekler (tipik zincir: `0029`–`0033` confluence / on-chain / Nansen genişletme; tam dosya adları dalda farklı olabilir — **`docs/QTSS_CURSOR_DEV_GUIDE.md` §6** ve `ls migrations`). Worker’ın `list_enabled_engine_symbols` sorgusu için **`0034`**, gerekirse **`0035`**; `to_regclass('public.bar_intervals')` NULL ise **`0036_bar_intervals_repair_if_missing.sql`**. **Tek önek kuralı** aynı §6’da.

Özet şema (0030 çekirdek; tüm kolonlar için depodaki güncel `CREATE`/`ALTER` zincirine bakın):

```sql
CREATE TABLE onchain_signal_scores (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol          TEXT NOT NULL,         -- 'BTCUSDT' veya '_global'
    computed_at     TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- Bireysel skorlar (her biri -1.0 .. +1.0)
    funding_score       DOUBLE PRECISION,
    oi_score            DOUBLE PRECISION,
    ls_ratio_score      DOUBLE PRECISION,
    taker_vol_score     DOUBLE PRECISION,
    exchange_netflow_score DOUBLE PRECISION,
    exchange_balance_score DOUBLE PRECISION,
    hl_bias_score       DOUBLE PRECISION,
    hl_whale_score      DOUBLE PRECISION,
    liquidation_score   DOUBLE PRECISION,
    nansen_sm_score     DOUBLE PRECISION,
    tvl_trend_score     DOUBLE PRECISION,

    -- Ağırlıklı birleşik skor
    aggregate_score     DOUBLE PRECISION NOT NULL,
    confidence          DOUBLE PRECISION NOT NULL DEFAULT 0.5,
    direction           TEXT NOT NULL CHECK (direction IN
                        ('strong_buy','buy','neutral','sell','strong_sell')),

    -- Rejim bağlamı (signal_dashboard'dan)
    market_regime       TEXT,              -- RANGE | TREND | KOPUS | BELIRSIZ

    -- Çelişki bilgisi
    conflict_detected   BOOLEAN NOT NULL DEFAULT false,
    conflict_detail     TEXT,

    -- Ham snapshot referansları
    snapshot_keys       TEXT[] NOT NULL DEFAULT '{}',
    meta_json           JSONB
);

CREATE INDEX idx_ocs_symbol_time ON onchain_signal_scores (symbol, computed_at DESC);

COMMENT ON TABLE onchain_signal_scores IS
  'On-chain + türev sinyal skorları. SignalScorer (qtss-worker) her tick''te yazar.';
```

---

## 4. Sinyal skorlama mantığı

### 4.1 Bireysel skor fonksiyonları

Her kaynak için bir `score_fn` JSON snapshot'ını okur ve -1.0 (bearish) ile +1.0 (bullish) arası skor döner.

| Kaynak | Skor mantığı |
|--------|-------------|
| `binance_funding_btc` | funding > +0.05% → -0.8 (aşırı long, kontra); funding < -0.03% → +0.8; -0.01%..+0.01% → 0.0 |
| `coinglass_oi` | OI artıyor + fiyat yükseliyor → +0.5; OI artıyor + fiyat düşüyor → -0.5; OI düşüyor → 0.0 (pozisyon kapanışı) |
| `binance_ls_ratio` | Long/short > 2.0 → -0.6 (kalabalık long); < 0.5 → +0.6; 0.8..1.2 → 0.0 |
| `binance_taker_vol` | Taker buy > taker sell → +0.4..+0.8; tersi → -0.4..-0.8 |
| `coinglass_netflow` | Büyük pozitif netflow (borsaya giriş) → -0.7; büyük negatif (çıkış) → +0.7 |
| `coinglass_exchange_balance` | Balance azalıyor → +0.5 (biriktirme); artıyor → -0.5 (satış hazırlığı) |
| `hl_meta` | OI ağırlıklı long/short bias → kontra sinyal; aşırı long bias → -0.6 |
| `hl_whale_positions` | Whale net long → +0.5; net short → -0.5 |
| `coinglass_liq` | Son 1h long liq > short liq → -0.5 (long flush); tersi → +0.5 (short squeeze) |
| `nansen_snapshots` | Smart money net alım → +0.6; net satım → -0.6; veri yoksa → 0.0 |
| `defillama_tvl_top` | TVL 24h trendi yükselen → +0.2; düşen → -0.2 (zayıf gösterge) |

### 4.2 Aggregate (ağırlıklı toplam)

```
aggregate_score = Σ(score_i × weight_i × confidence_i) / Σ(weight_i × confidence_i)
```

`confidence_i`: snapshot yaşına göre düşer. `computed_at` < 2 × `tick_secs` → 1.0; 2-5× → 0.5; 5×+ → 0.1 (bayat veri).

Ağırlıklar rejime göre değişir (bkz. §5).

### 4.3 Yön belirleme

| aggregate_score | direction |
|----------------|-----------|
| +0.6..+1.0 | strong_buy |
| +0.2..+0.6 | buy |
| -0.2..+0.2 | neutral |
| -0.6..-0.2 | sell |
| -1.0..-0.6 | strong_sell |

### 4.4 Çelişki tespiti

Teknik sinyaller (signal_dashboard.durum) ile on-chain aggregate zıt yönleri gösteriyorsa `conflict_detected = true` ve pozisyon gücü düşürülür. Örnek: signal_dashboard "LONG" + aggregate_score < -0.3 → çelişki.

---

## 5. Rejime göre ağırlık tablosu

Mevcut `signal_dashboard.piyasa_modu` (RANGE | TREND | KOPUS | BELIRSIZ) rejim belirler. Her rejim farklı ağırlık seti kullanır.

### 5.1 RANGE (yatay piyasa)

ATR < ATR_SMA, fiyat bant içinde.

| Katman | Ağırlık | Birincil modüller |
|--------|---------|-------------------|
| Teknik | %50 | Trading range, sweep sinyalleri, ACP channel patterns |
| On-chain | %35 | Funding rate (kontra), likidasyon haritası, exchange netflow |
| Smart money | %15 | Nansen (tamamlayıcı) |

Trading range ve sweep burada en güvenilir sinyal. Funding aşırı pozitifken range üst bandına yaklaşma → short sweep beklentisi. ACP kanal formasyonu range sınırlarını doğrular. Elliott dalga sayımı güvenilmez çünkü düzeltme dalgaları birçok farklı biçimde sayılabilir — Elliott skorunu RANGE'de sıfırlayın.

### 5.2 TREND (güçlü yön)

ATR ≥ ATR_SMA, yerel_trend YUKARI veya ASAGI.

| Katman | Ağırlık | Birincil modüller |
|--------|---------|-------------------|
| Teknik | %30 | Elliott wave (impulse), signal_dashboard trend |
| On-chain | %40 | DEX/taker buy-sell, CEX netflow, whale hareketleri, OI |
| Smart money | %30 | Nansen, HL whale positions |

Elliott dalga sayımı trend piyasalarda en güçlüdür — impulse dalgaları (1-3-5) net yapıdadır. Dalga 3 içinde trend devam beklenirken, on-chain veriler (taker alım baskısı, borsadan çıkış, whale biriktirme) bunu destekliyorsa pozisyon güçlendirilir. Trading range burada anlamsız çünkü fiyat sürekli yeni seviye yapıyor.

### 5.3 KOPUS (breakout)

Sweep sinyali aktif, fiyat bandı kırdı.

| Katman | Ağırlık | Birincil modüller |
|--------|---------|-------------------|
| Teknik | %40 | Trading range (sweep), ACP (breakout doğrulama) |
| On-chain | %45 | OI değişimi, likidasyon kaskadı, HL bias, taker volume |
| Smart money | %15 | Nansen (tamamlayıcı) |

Kopuşun gerçek mi yoksa fake mi olduğunu on-chain belirler: OI hızla artıyor + likidasyon tetiklendi → gerçek kopuş. ACP kanal kırılımı doğrularsa güven artar. Elliott dalga sayımı kopuşun dalga 1 mi (devam edecek) yoksa dalga 5 sonu mu (geri dönecek) olduğunu söyler.

### 5.4 BELIRSIZ

Net trend yok, range de değil.

| Katman | Ağırlık | Birincil modüller |
|--------|---------|-------------------|
| Teknik | %20 | (Düşük güvenilirlik) |
| On-chain | %30 | On-chain confluence skoru |
| Smart money | %50 | Nansen, HL whale positions |

Teknik sinyaller çelişiyor ama on-chain veriler net yön gösteriyorsa (smart money agresif alım + funding negatif) düşük pozisyon ile on-chain yönüne girilebilir. Hiçbir katman net değilse → kenarda kal (pozisyon boyutu sıfır veya minimum).

### 5.5 Ağırlık konfigürasyonu

`app_config` tablosunda JSON olarak saklanır. Worker restart gerektirmez.

```json
{
  "RANGE":    {"teknik": 0.50, "onchain": 0.35, "smart_money": 0.15},
  "TREND":    {"teknik": 0.30, "onchain": 0.40, "smart_money": 0.30},
  "KOPUS":    {"teknik": 0.40, "onchain": 0.45, "smart_money": 0.15},
  "BELIRSIZ": {"teknik": 0.20, "onchain": 0.30, "smart_money": 0.50}
}
```

Admin: `POST /api/v1/config` ile güncellenir (`key = "onchain_signal_weights"`). Varsayılan seed: **`0031_onchain_signal_weights_app_config.sql`** — JSON alanları **bileşen** ağırlıklarıdır (`taker`, `funding`, `oi`, `ls_ratio`, …); §5.1–5.4’teki sütun/onchain-oran tablosu **confluence** için `confluence_weights_by_regime` anahtarına bakın (`0025_confluence_weights_app_config.sql`).

---

## 6. Mevcut modüllerle entegrasyon

### 6.1 engine_analysis loop'una ekleme

Mevcut `run_engines_for_symbol` fonksiyonu her `engine_symbol` için `trading_range` + `signal_dashboard` üretiyor. Confluence katmanı aynı loop'a eklenir:

```
run_engines_for_symbol(pool, bar_limit, params):
    for t in engine_symbols:
        1. trading_range = analyze_trading_range(bars, params)        [MEVCUT]
        2. signal_dashboard = compute_signal_dashboard_v1(bars, tr)   [MEVCUT]
        3. onchain_scores = read_latest_signal_scores(pool, symbol)   [YENİ]
        4. confluence = compute_confluence(
               signal_dashboard,
               onchain_scores,
               market_regime = signal_dashboard.piyasa_modu
           )                                                          [YENİ]
        5. upsert_analysis_snapshot(pool, t.id, "confluence", ...)    [YENİ]
```

### 6.2 signal_dashboard v2 — pozisyon_gucu_10 genişletme

Mevcut `pozisyon_gucu_10` hesaplaması (0-10 arası skor) on-chain katkısı ile güçlendirilir:

```
-- Mevcut skor (teknik): max 10
-- On-chain katkısı:
-- confluence.direction = strong_buy/strong_sell → +2
-- confluence.direction = buy/sell → +1
-- confluence.conflict_detected = true → -2
-- Yeni maks: 12 (normalize → 10)
```

### 6.3 Bildirim entegrasyonu

Mevcut `qtss-notify` altyapısı (Telegram, Discord, e-posta, SMS, webhook) eşik aşımı bildirimlerini doğrudan destekler. Yeni bildirim kanalı yazmaya gerek yok.

```
if aggregate_score.abs() > 0.6 && !conflict_detected:
    NotificationDispatcher::send(channels, Notification {
        title: format!("{} — {}", symbol, direction),
        body: format!("Skor: {:.2}, Rejim: {}, Güven: {:.0}%",
                       aggregate_score, market_regime, confidence * 100.0),
    })
```

Bildirim tetikleme: `QTSS_NOTIFY_ON_ONCHAIN_SIGNAL=1` ve `QTSS_NOTIFY_ON_ONCHAIN_SIGNAL_CHANNELS=telegram,discord`.

### 6.4 Elliott wave ile ilişki

Elliott dalga sayımı (`web/src/lib/elliottEngineV2/`) şu an client-side çalışıyor. Sunucu tarafında impulse/corrective tespiti yapılırsa:

- TREND rejiminde impulse dalga 3 tespiti → teknik skor +0.8
- TREND rejiminde dalga 5 sonu tespiti → teknik skor -0.3 (trend tükenme)
- RANGE rejiminde → Elliott skorunu sıfırla (düzeltme dalga sayımı güvenilmez)

### 6.5 ACP channel patterns ile ilişki

`analyze_channel_six_from_bars` formasyonları tespit ediyor. Confluence'a katkısı:

- RANGE'de kanal formasyonu tespiti → range sınırlarını doğrular, teknik güven +0.3
- KOPUS'ta kanal kırılımı → kopuşun gerçekliğini teyit, teknik güven +0.4
- TREND'de → ACP katkısı düşük (kanal oluşumu için yatay hareket gerekir)

---

## 7. API endpoint'leri

### 7.1 Yeni route'lar

`crates/qtss-api/src/routes/onchain_signals.rs` — `analysis_read_router`'a eklenir.

| Yol | Açıklama | Rol |
|-----|----------|-----|
| `GET /api/v1/analysis/onchain-signals/latest?symbol=BTCUSDT` | Son birleşik skor + tüm alt skorlar | viewer+ |
| `GET /api/v1/analysis/onchain-signals/history?symbol=BTCUSDT&limit=100` | Skor geçmişi (dashboard grafik) | viewer+ |
| `GET /api/v1/analysis/onchain-signals/breakdown?symbol=BTCUSDT` | Her kaynağın son skoru + güven + ağırlık | viewer+ |
| `GET /api/v1/analysis/confluence/latest?symbol=BTCUSDT` | Confluence skoru (teknik + on-chain + smart money birleşimi) | viewer+ |

### 7.2 Mevcut endpoint genişletmeleri

| Yol | Değişiklik |
|-----|-----------|
| `GET /api/v1/analysis/engine/snapshots` | `engine_kind = "confluence"` eklenir |
| `GET /api/v1/dashboard/pnl` | Confluence skor grafik verisi opsiyonel |

---

## 8. Worker modülleri

### 8.1 Yeni dosya: `crates/qtss-worker/src/onchain_signal_scorer.rs`

```
pub async fn onchain_signal_loop(pool: PgPool) {
    // Tick: QTSS_ONCHAIN_SIGNAL_TICK_SECS (varsayılan 60)
    loop {
        // 1. external_data_snapshots oku (tüm source_key'ler)
        // 2. nansen_snapshots oku
        // 3. Her key için score_fn dispatch
        // 4. Mevcut signal_dashboard snapshot'dan piyasa_modu al
        // 5. Rejime göre ağırlık tablosunu seç (app_config'den)
        // 6. aggregate() hesapla
        // 7. Çelişki tespiti (teknik vs on-chain)
        // 8. onchain_signal_scores tablosuna yaz
        // 9. Eşik aşılırsa → NotificationDispatcher::send()
        tokio::time::sleep(Duration::from_secs(tick)).await;
    }
}
```

### 8.2 main.rs değişikliği

```rust
// Mevcut spawn'lar arasına eklenir:
let onchain_pool = pool.clone();
tokio::spawn(onchain_signal_scorer::onchain_signal_loop(onchain_pool));
```

### 8.3 Ortam değişkenleri (yeni)

| Değişken | Varsayılan | Açıklama |
|----------|-----------|----------|
| `QTSS_ONCHAIN_SIGNAL_TICK_SECS` | 60 | Scorer loop tick aralığı (saniye) |
| `QTSS_ONCHAIN_SIGNAL_WEIGHTS_KEY` | `onchain_signal_weights` | app_config'deki ağırlık JSON key |
| `QTSS_NOTIFY_ON_ONCHAIN_SIGNAL` | 0 | 1 ise eşik aşımında bildirim gönderir |
| `QTSS_NOTIFY_ON_ONCHAIN_SIGNAL_CHANNELS` | webhook | Virgülle ayrılmış kanal listesi |
| `QTSS_ONCHAIN_NOTIFY_THRESHOLD` | 0.6 | Bildirim tetikleme eşiği (aggregate_score.abs()) |

---

## 9. Uygulama adımları

### Faz 1 — Veri toplama

1. Migration’lar `0021`+ — `external_data_sources` seed + `data_snapshots` yazımı
2. Worker restart veya bir sonraki tick'i bekle
3. `GET /api/v1/analysis/external-fetch/snapshots` ile veri geldiğini doğrula

### Faz 2 — Skorlama motoru

4. Migration **`0030_onchain_signal_scores.sql`** — tablo; ardından **`0031`+** `app_config` seed ve **`0029`–`0033`** aralığındaki on-chain / confluence / Nansen migrasyonları (dosya adları için §6); **`0034` / `0035` / `0036`** — `engine_symbols` FK + eksik `bar_intervals` telafisi (§6). §6 tek önek kuralı.
5. `crates/qtss-worker/src/onchain_signal_scorer.rs` — scorer loop
6. `main.rs`'e `tokio::spawn` ekle
7. `crates/qtss-storage/src/` — yeni CRUD fonksiyonları (`upsert_onchain_signal_score`, `fetch_latest_signal_score`)

### Faz 3 — API ve confluence

8. `crates/qtss-api/src/routes/onchain_signals.rs` — üç GET endpoint
9. `engine_analysis.rs`'e confluence hesaplama entegrasyonu
10. `signal_dashboard_v1.rs` veya v2 — `pozisyon_gucu_10`'a on-chain katkısı

### Faz 4 — Bildirim ve dashboard

11. Scorer loop'una `NotificationDispatcher` entegrasyonu
12. Web dashboard: Piyasa bağlamı sekmesinde on-chain özet (`GET …/onchain-signals/breakdown`)
13. Grafik üzerinde on-chain göstergeleri (isteğe bağlı sonraki adım)

---

## 10. Güvenlik ve operasyonel notlar

- API key'ler (Coinglass, Nansen) `external_data_sources.headers_json` veya `exchange_accounts` tablosunda; üretimde KMS/Vault hedefi (mevcut güvenlik politikası ile tutarlı).
- Rate limit: Coinglass ücretsiz plan aylık 10.000 API çağrısı (günde ~330). 300s tick ile günde ~288 çağrı yapılır — limit dahilinde. Binance FAPI 1200 req/dk, Hyperliquid limitsiz.
- Snapshot temizleme: `onchain_signal_scores` tablosunda 7 günden eski satırlar periyodik silinir (cron veya worker task).
- Bayat veri koruması: snapshot `computed_at` yaşı `2 × tick_secs`'i aşarsa `confidence` düşer; `5 × tick_secs`'i aşarsa skor hesaplamaya dahil edilmez.

---

*Son güncelleme: Bu belge proje deposu ile eşlik eder; sinyal kaynakları veya skorlama mantığı değiştikçe gözden geçirilmelidir.*
