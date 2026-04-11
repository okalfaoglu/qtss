# Faz 10 — Wyckoff Full Implementation

> Status: Planned · 2026-04-11
> Referans: *Wyckoff 2.0 — Structures, Volume Profile and Order Flow* (Villahermosa)
> Bağımlılık: Faz 2 (mevcut 3 event), Faz 3 (validator)

---

## 1. Hedef

Kitaptaki **tüm Wyckoff yapılarını** (Accumulation, Distribution, Re-accumulation, Re-distribution) algılamak, Phase A–E sınıflandırması yapmak ve GUI'de **yapı diyagramı + faz etiketi + olay dizisi** ile görselleştirmek.

---

## 2. Mevcut Durum

| Bileşen | Durum |
|---------|-------|
| `qtss-wyckoff` crate | 3 event: trading_range, spring, upthrust |
| EventSpec dispatch table | Genişletilebilir (`&[EventSpec]` slice) |
| WyckoffConfig | 9 parametre, system_config'den okunur |
| GUI | Generic detection tablo + chart'ta mor polyline |
| Phase/Event sınıflandırma | Yok |
| Volume Profile entegrasyonu | Yok |

---

## 3. Mimari Tasarım

### 3A. Wyckoff Structure State Machine

Her sembol/timeframe için bir `WyckoffStructure` durum makinesi:

```
Phase A (Stopping)
  → PS (Preliminary Support/Supply)
  → SC/BC (Selling Climax / Buying Climax)
  → AR (Automatic Rally / Automatic Reaction)
  → ST (Secondary Test)

Phase B (Building the Cause)
  → UA (Upthrust Action) — distribution'da
  → ST-B (Secondary Tests in Phase B)
  → Range bound, volume decreasing

Phase C (Test)
  → Spring (accumulation) / UTAD (distribution)
  → Shakeout / Terminal Shakeout

Phase D (Trend within Range)
  → SOS (Sign of Strength) / SOW (Sign of Weakness)
  → LPS (Last Point of Support) / LPSY (Last Point of Supply)
  → JAC (Jump Across Creek) / Break of Ice
  → BUEC (Back-Up to Edge of Creek)

Phase E (Trend out of Range)
  → Markup / Markdown
```

### 3B. Yapı Tipleri

```rust
enum WyckoffSchematic {
    Accumulation,       // Klasik birikim
    Distribution,       // Klasik dağıtım
    ReAccumulation,     // Trend içi yeniden birikim (başarısız dağıtım)
    ReDistribution,     // Trend içi yeniden dağıtım (başarısız birikim)
}

enum WyckoffPhase { A, B, C, D, E }

enum WyckoffEvent {
    PS, SC, BC, AR, ST,           // Phase A
    UA, STB,                       // Phase B
    Spring, UTAD, Shakeout,        // Phase C
    SOS, SOW, LPS, LPSY,          // Phase D
    JAC, BreakOfIce, BUEC,        // Phase D (creek/ice)
    SOT,                           // Shortening of Thrust
    Markup, Markdown,              // Phase E
}
```

---

## 4. Uygulama Adımları

### Adım 1: Phase A Olayları (PS, SC/BC, AR, ST)

**Dosya:** `crates/qtss-wyckoff/src/events.rs` — yeni eval fonksiyonları

| Olay | Algılama Kriteri (Kitaptan) |
|------|---------------------------|
| **PS** (Preliminary Support) | Uzun düşüş trendinden sonra ilk belirgin hacim artışı + destek reaksiyonu. Trend hâlâ kırılmadı. |
| **SC** (Selling Climax) | Panik satışı — en yüksek hacim + en geniş bar + kapanış destek altı veya yakını. Önceki ATR'nin 2-3x'i. |
| **BC** (Buying Climax) | SC'nin aynası — panik alımı, en yüksek hacim, direnç üstü spike. |
| **AR** (Automatic Rally/Reaction) | SC/BC sonrası ilk otomatik tepki hareketi. Hacim düşer, fiyat zıplar/düşer. Range'in karşı ucunu belirler. |
| **ST** (Secondary Test) | SC/BC seviyesine geri test. Hacim < SC/BC hacmi. Fiyat SC/BC'nin altına inmez (veya kısa süre iner). |

**Config parametreleri** (system_config):
- `wyckoff.sc_volume_multiplier` (default: 2.5) — SC hacminin önceki ortalamaya oranı
- `wyckoff.sc_bar_width_multiplier` (default: 2.0) — SC bar genişliğinin ATR'ye oranı
- `wyckoff.st_max_volume_ratio` (default: 0.7) — ST hacminin SC hacmine max oranı
- `wyckoff.ar_min_retracement` (default: 0.3) — AR'nin SC→önceki direnç mesafesinin min geri çekilme oranı

### Adım 2: Phase B Olayları (UA, ST-B)

| Olay | Algılama Kriteri |
|------|-----------------|
| **UA** (Upthrust Action) | Phase B'de AR seviyesi üstüne kısa süreli çıkış, kapanış range içinde. Distribution habercisi. |
| **ST-B** | Phase B içinde SC/BC seviyesine yapılan ek testler. Her test hacim < önceki test. |

**Config:**
- `wyckoff.ua_max_exceed_pct` (default: 0.03) — AR üstü aşım yüzdesi
- `wyckoff.stb_volume_decay_min` (default: 0.85) — her ardışık ST-B'de hacim azalma oranı

### Adım 3: Phase C Olayları (Spring, UTAD, Shakeout) — MEVCUT + genişletme

Mevcut `eval_spring` ve `eval_upthrust` Phase C olayları. Ek:
- **Shakeout** — Spring'den daha derin ve hacimli penetrasyon, hızlı geri alım
- **Terminal Shakeout** — range'in en düşük noktası, panik hacmi, ardından güçlü toparlanma

**Config:**
- `wyckoff.shakeout_min_penetration` (default: 0.05)
- `wyckoff.shakeout_recovery_bars` (default: 3)

### Adım 4: Phase D Olayları (SOS, SOW, LPS, LPSY, JAC, BUEC)

| Olay | Algılama Kriteri |
|------|-----------------|
| **SOS** (Sign of Strength) | Spring/Test sonrası güçlü hacimli yükseliş, AR seviyesini aşar veya yaklaşır |
| **SOW** (Sign of Weakness) | UTAD sonrası güçlü hacimli düşüş, SC seviyesine iner |
| **LPS** (Last Point of Support) | SOS sonrası geri çekilme, sığ ve düşük hacimli. Creek üstünde kalır. |
| **LPSY** (Last Point of Supply) | SOW sonrası rally, zayıf ve düşük hacimli. Ice altında kalır. |
| **JAC** (Jump Across Creek) | Range'in üst yarısını kıran güçlü hacimli hareket (accumulation) |
| **Break of Ice** | Range'in alt yarısını kıran güçlü hacimli hareket (distribution) |
| **BUEC** (Back-Up to Edge of Creek) | JAC sonrası creek'e geri çekilme, düşük hacimli. Son alım fırsatı. |

**Config:**
- `wyckoff.sos_min_volume_ratio` (default: 1.5)
- `wyckoff.lps_max_retracement` (default: 0.5) — SOS tepesinin max geri çekilme oranı
- `wyckoff.lps_max_volume_ratio` (default: 0.5)
- `wyckoff.creek_level_percentile` (default: 0.6) — range'in creek seviyesi yüzdeliği

### Adım 5: Creek / Ice Seviyeleri

**Creek**: Accumulation yapısında range'in üst yarısında oluşan direnç çizgisi (AR ve UA tepelerini bağlayan çizgi).
**Ice**: Distribution yapısında range'in alt yarısında oluşan destek çizgisi (SC ve ST diplerini bağlayan çizgi).

```rust
struct CreekIce {
    creek_level: f64,    // Accumulation resistance trendline
    ice_level: f64,      // Distribution support trendline
    slope: f64,          // Eğim (sloping structures)
}
```

### Adım 6: SOT (Shortening of Thrust)

Her ardışık SOS/SOW hareketinin **momentum azalması** tespiti. Price thrust distance küçülüyorsa → trend zayıflıyor.

### Adım 7: Sloping Structures

Kitabın önemli konusu: **eğik** range'ler (yukarı eğimli accumulation, aşağı eğimli distribution).
- Range tespitinde `edge_tolerance` yerine **linear regression** ile eğim hesaplanır
- Eğim > threshold → sloping structure olarak etiketlenir

### Adım 8: Yapı Başarısızlığı (Structural Failure)

- Accumulation yapısının distribution'a dönüşmesi (Phase C testi başarısız)
- Distribution yapısının re-accumulation'a dönüşmesi
- Tetikleyici: Phase C olayı beklenen yönde tamamlanmaz → yapı tipi güncellenir

### Adım 9: Phase State Machine

```rust
struct WyckoffStructureTracker {
    schematic: WyckoffSchematic,
    current_phase: WyckoffPhase,
    events: Vec<(WyckoffEvent, PivotIndex, f64)>,  // event, pivot, score
    range_top: f64,       // AR / BC level
    range_bottom: f64,    // SC / Spring level
    creek: Option<f64>,
    ice: Option<f64>,
    volume_profile: Option<VolumeProfileData>,
}
```

Her yeni bar/pivot geldiğinde tracker güncellenir. Phase geçişleri:
- A→B: ST tamamlandıktan sonra
- B→C: Yeterli range süresi + hacim azalması
- C→D: Spring/UTAD + test başarılı
- D→E: JAC/BreakOfIce + BUEC tamamlandı

### Adım 10: Volume Profile Entegrasyonu

Kitabın ana tezlerinden biri: **Volume Profile ile Wyckoff doğrulama**.
- Range içi volume distribution (POC, VA, HVN, LVN)
- POC range'in alt yarısında → accumulation teyidi
- POC range'in üst yarısında → distribution teyidi
- Volume holes → breakout hedef bölgeleri

**Bağımlılık:** `qtss-indicators` crate'ine Volume Profile hesaplaması eklenmeli.

---

## 5. GUI Tasarımı

### 5A. Wyckoff Structure Overlay (Chart)

Chart üzerine çizilecekler:
1. **Range Box** — SC-AR arasındaki yatay (veya eğimli) kutu, yarı saydam dolgu
2. **Creek / Ice çizgisi** — kesikli çizgi, eğimli olabilir
3. **Event Labels** — her tespit edilen olay noktasında etiket (PS, SC, AR, ST, Spring, SOS, LPS, JAC, BUEC vb.)
4. **Phase Bands** — range kutusunun altında renkli bantlar (A=kırmızı, B=turuncu, C=sarı, D=yeşil, E=mavi)
5. **Volume Profile Histogram** — range sol tarafında yatay hacim çubukları

### 5B. Wyckoff Structure Panel (Sidebar)

```
┌─────────────────────────────────────┐
│  WYCKOFF STRUCTURE                  │
│  ─────────────────                  │
│  Tip: Accumulation                  │
│  Faz: D (Trend within Range)        │
│  Güvenilirlik: 78%                  │
│                                     │
│  Olay Dizisi:                       │
│  ✅ PS   — 2026-04-01  $2,450      │
│  ✅ SC   — 2026-04-03  $2,280      │
│  ✅ AR   — 2026-04-05  $2,520      │
│  ✅ ST   — 2026-04-08  $2,310      │
│  ✅ Spring — 2026-04-10 $2,260     │
│  ✅ SOS  — 2026-04-11  $2,540      │
│  🔄 LPS  — Bekleniyor...           │
│  ⬜ JAC  — Bekleniyor...           │
│  ⬜ BUEC — Bekleniyor...           │
│                                     │
│  Creek: $2,520                      │
│  Range: $2,280 – $2,520            │
│  Volume POC: $2,380 (alt yarı ✅)  │
│                                     │
│  Sinyal: 🟢 LONG after LPS         │
│  Hedef: $2,650 (Volume Hole)        │
│  Stop: $2,250 (Spring altı)         │
└─────────────────────────────────────┘
```

### 5C. Wyckoff Schematic Reference Diagram

Kullanıcı yapıyı anlasın diye **referans diyagram**:
- Statik SVG: Klasik Accumulation ve Distribution şeması
- Mevcut faz highlight edilir
- Tıklanınca kitaptaki açıklama tooltip'de gösterilir

### 5D. Wyckoff Dedicated Page (`/v2/wyckoff`)

| Bölüm | İçerik |
|--------|--------|
| Aktif Yapılar Tablosu | Tüm semboller için tespit edilen aktif Wyckoff yapıları (tip, faz, güvenilirlik, son olay) |
| Tarihsel Yapılar | Tamamlanmış ve başarısız yapıların listesi (backtest doğrulama ile) |
| Yapı Detay | Tıklanan yapının chart overlay + event timeline + volume profile |
| Schematic Reference | Accumulation / Distribution referans diyagramları |
| Config Panel | Tüm Wyckoff parametrelerinin canlı düzenleme |

---

## 6. Migration'lar

### 6A. Config Seed

```sql
-- 0050_wyckoff_phase_config.sql
INSERT INTO system_config (module, config_key, value, description) VALUES
  -- Phase A
  ('detector', 'wyckoff.sc_volume_multiplier',    '2.5',  'SC/BC volume must be >= Nx avg volume'),
  ('detector', 'wyckoff.sc_bar_width_multiplier',  '2.0',  'SC/BC bar range >= Nx ATR'),
  ('detector', 'wyckoff.st_max_volume_ratio',      '0.7',  'ST volume must be <= Nx SC volume'),
  ('detector', 'wyckoff.ar_min_retracement',       '0.3',  'AR must retrace >= N% of SC drop'),
  -- Phase B
  ('detector', 'wyckoff.ua_max_exceed_pct',        '0.03', 'UA may exceed AR by at most N%'),
  ('detector', 'wyckoff.stb_volume_decay_min',     '0.85', 'Each ST-B volume <= N * prev ST-B'),
  -- Phase C
  ('detector', 'wyckoff.shakeout_min_penetration',  '0.05', 'Shakeout penetration >= N% of range'),
  ('detector', 'wyckoff.shakeout_recovery_bars',     '3',   'Shakeout must recover within N bars'),
  -- Phase D
  ('detector', 'wyckoff.sos_min_volume_ratio',      '1.5', 'SOS volume >= Nx average'),
  ('detector', 'wyckoff.lps_max_retracement',       '0.5', 'LPS retracement <= N% of SOS move'),
  ('detector', 'wyckoff.lps_max_volume_ratio',      '0.5', 'LPS volume <= N% of SOS volume'),
  ('detector', 'wyckoff.creek_level_percentile',    '0.6', 'Creek = N% of range height'),
  -- Sloping
  ('detector', 'wyckoff.slope_threshold_deg',       '5.0', 'Range slope > Ndeg = sloping structure'),
  -- SOT
  ('detector', 'wyckoff.sot_thrust_decay_ratio',    '0.7', 'Each thrust <= N * prev thrust = SOT')
ON CONFLICT (module, config_key) DO NOTHING;
```

### 6B. Wyckoff Structures Tablosu

```sql
-- 0051_wyckoff_structures_table.sql
CREATE TABLE IF NOT EXISTS wyckoff_structures (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol          TEXT NOT NULL,
    interval        TEXT NOT NULL,
    exchange        TEXT NOT NULL DEFAULT 'binance',
    segment         TEXT NOT NULL DEFAULT 'futures',
    schematic       TEXT NOT NULL,        -- accumulation, distribution, reaccumulation, redistribution
    current_phase   TEXT NOT NULL,        -- A, B, C, D, E
    range_top       DOUBLE PRECISION,
    range_bottom    DOUBLE PRECISION,
    creek_level     DOUBLE PRECISION,
    ice_level       DOUBLE PRECISION,
    slope_deg       DOUBLE PRECISION DEFAULT 0,
    confidence      DOUBLE PRECISION DEFAULT 0,
    events_json     JSONB NOT NULL DEFAULT '[]',  -- [{event, pivot_ts, price, score}]
    volume_profile  JSONB,                         -- {poc, va_high, va_low, hvn, lvn}
    is_active       BOOLEAN DEFAULT true,
    started_at      TIMESTAMPTZ NOT NULL,
    completed_at    TIMESTAMPTZ,
    failed_at       TIMESTAMPTZ,
    failure_reason  TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_wyckoff_structures_active ON wyckoff_structures (symbol, interval) WHERE is_active = true;
CREATE INDEX idx_wyckoff_structures_symbol ON wyckoff_structures (symbol, started_at DESC);
```

---

## 7. API Endpoint'leri

| Method | Path | Açıklama |
|--------|------|----------|
| GET | `/v2/wyckoff/active` | Tüm aktif yapılar (filtrelenebilir) |
| GET | `/v2/wyckoff/structure/{id}` | Tek yapı detayı (events + volume profile) |
| GET | `/v2/wyckoff/history/{symbol}` | Sembolün tarihsel yapıları |
| GET | `/v2/wyckoff/overlay/{symbol}/{interval}` | Chart overlay verisi (range box, creek/ice, labels) |

---

## 8. Test Planı

- **Unit tests**: Her event eval fonksiyonu için en az 3 sennaryo (tespit, tespit etmeme, edge case)
- **State machine tests**: Phase geçişleri (A→B→C→D→E), yapı başarısızlığı, re-classification
- **Integration test**: Tarihsel BTCUSDT/ETHUSDT verisiyle bilinen Wyckoff yapılarının tespiti
- **GUI E2E**: Puppeteer ile chart overlay + panel render doğrulaması

---

## 9. Tahmini Efor

| Adım | Efor |
|------|------|
| Phase A events (PS, SC, AR, ST) | 1 gün |
| Phase B events (UA, ST-B) | 0.5 gün |
| Phase C genişletme (Shakeout) | 0.5 gün |
| Phase D events (SOS, SOW, LPS, LPSY, JAC, BUEC) | 1.5 gün |
| Creek/Ice + SOT + Sloping | 1 gün |
| State machine + tracker | 1 gün |
| Yapı başarısızlığı | 0.5 gün |
| Volume Profile entegrasyonu | 1 gün |
| DB + API + migration | 0.5 gün |
| GUI: Chart overlay | 1.5 gün |
| GUI: Structure panel + dedicated page | 1.5 gün |
| GUI: Schematic reference SVG | 0.5 gün |
| Testler | 1 gün |
| **TOPLAM** | **~11 gün** |
