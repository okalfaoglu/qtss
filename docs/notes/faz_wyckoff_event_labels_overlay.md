# BACKLOG — Wyckoff event labels chart overlay

Durum: **backlog** (4 ignored Wyckoff testi bugfix sonrası).
Tarih: 2026-04-19

## Sorun
Wyckoff yapısının canonical event'leri detector tarafından üretilip
`qtss_v2_detections` + `qtss_wyckoff_structures.events_json` içine
yazılıyor; ancak GUI grafik ekranında (`/v2/chart`) **etiketlenmiyor**.
Operator yapı içinde nerede SC, AR, ST, SOS, Spring vb. olduğunu
göremiyor — sadece `/v2/wyckoff` tablosundan okuyabiliyor.

## Etiketlenecek event seti
Phase'lere göre canonical Wyckoff vocabulary (Villahermosa ch. 4-6):

| Phase | Accumulation | Distribution |
|---|---|---|
| **A** (durdurma) | PS, SC, AR, ST | PSY, BC, AR, ST |
| **B** (yapı) | TEST, secondary tests | UT, secondary tests |
| **C** (manipülasyon) | Spring, SpringTest, Shakeout | UTAD, UT |
| **D** (trend yönü) | SOS, LPS, JAC, BUEC | SOW, LPSY, BreakOfIce |
| **E** (markup/markdown) | Markup | Markdown |

Kısaltma açıklamaları operator için tooltip:
- **PS** Preliminary Support / **PSY** Preliminary Supply
- **SC** Selling Climax / **BC** Buying Climax
- **AR** Automatic Rally/Reaction
- **ST** Secondary Test / **STB** ST in Phase B
- **UA** Upthrust After distribution
- **UTAD** Upthrust After Distribution
- **SOS** Sign of Strength / **SOW** Sign of Weakness
- **LPS** Last Point of Support / **LPSY** Last Point of Supply
- **JAC** Jump Across the Creek
- **BUEC** Back-Up to Edge of Creek
- **BU/LPS** = LPS (alternative ad)

## Veri Yolu
1. `qtss_wyckoff_structures.events_json` zaten her tracker için tüm
   event'leri saklıyor (`{event, bar_index, price, score, time_ms}`).
2. `/v2/wyckoff` API'si bu rows'u dönüyor (tablo gösterimi için).
3. **Eksik:** Chart ekranı için bar_index/time → label projection yok.

## İş Adımları

### 1. API
Yeni endpoint veya mevcut `/v2/wyckoff` cevabını zenginleştir:
```
GET /v2/wyckoff/events?symbol=BTCUSDT&timeframe=1h&since=...
→ [{ ts, price, event_code (3-4 char), full_name, phase, schematic, score, struct_id }]
```
Mevcut row'ları flatten edip `events_json` array'ini açar.

### 2. Chart.tsx overlay
- Yeni state: `showWyckoffLabels` (default false), toggle chip "WYK".
- Yeni `useQuery` → `/v2/wyckoff/events` (symbol/timeframe debounced).
- lightweight-charts'a marker ekle: `series.setMarkers([...])`
  - `position`: long-yön event'leri (Spring, SOS, LPS) → `belowBar`,
    short-yön (UTAD, SOW, LPSY) → `aboveBar`, neutral (SC, AR, ST) → `inBar`
  - `shape`: `arrowUp` / `arrowDown` / `circle`
  - `color`: bullish family → emerald, bearish → red, neutral → zinc
  - `text`: kısa kod (SC, SOS, …)

### 3. Renk paleti (CLAUDE.md #1 — dispatch tablosu)
`Record<WyckoffEventCode, { color, position, shape }>` map; if/else
zincirine düşürmemek için detector'dan gelen `event` string'i direkt
key olarak kullan.

### 4. Tooltip
Marker üzerine hover → full name + phase + schematic + score + struct
linki (`/v2/wyckoff?struct=<uuid>`).

### 5. Performance
- Marker array'i 200 event'le sınırla (en yeni 200, eski struct'lar
  zaten Phase E'de kapanmış olur).
- WebSocket/SSE şart değil — 30s polling refetchInterval yeter,
  events_json yavaş değişiyor.

## Bonus
- Phase boundary'leri (A→B, B→C, …) düşey çizgi olarak ek katman
  (`PriceLine` yerine `LineSeries` + 2 nokta — Chart.tsx'te zaten
  benzer pattern var setup overlay'de).
- Schematic değişimleri (Accumulation → ReDistribution flip) farklı
  renkte düşey çizgi — operator regime flip'leri görsel olarak takip
  edebilir.

## Tahmini efor
- API endpoint + flatten: ~80 LOC
- Chart overlay + tooltip + marker map: ~200 LOC
- Toggle + state: ~20 LOC
- Toplam: ~5-6 saat (Setup overlay pattern'i hazır referans)

## Niye şimdi değil
4 ignored Wyckoff testi yeşillendi (commit 32e39f3) — sıradaki
operatör-görünür iyileştirme. Backtest dispatcher prod'a açıldıktan
sonra ne kadar event'in ekrana sığacağı netleşince başlanmalı (eğer
çok yoğunsa marker dedup / zoom-aware filtre gerekebilir).

## İlgili dosyalar
- `crates/qtss-wyckoff/src/structure.rs` — `WyckoffEvent` enum + serde rename map
- `crates/qtss-wyckoff/src/persistence.rs` — events_json yazımı
- `crates/qtss-api/src/routes/v2_wyckoff.rs` — mevcut endpoint
- `web/src/pages/Chart.tsx` — setup/position overlay pattern'i
- `web/src/pages/Wyckoff.tsx` — tablo görünümü (referans)
