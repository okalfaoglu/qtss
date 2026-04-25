# Major Dip / Impulse-Start Detection — Research Doc

User: "kafadan atma ve araştır. elliott itkinin yani major dipin
başladığını nasıl tespit ederiz. hangi değerler bize bilgi verir."

Bu doc kaynak göstererek bilgi listeliyor. Her madde Elliott literatürü,
piyasa mikro-yapısı veya empirik araştırmaya dayanıyor.

---

## Niye gerekli

QTSS şu an itki başlangıcını **mekanik olarak** belirliyor:
- Pine port 6-pivot pencere kayar, 1-2-3-4-5 kuralı tutuyorsa motive emit
  ediyor.
- IQ-D candidate loop, motive'nin W3 / W2 anchor'ını entry seviyesi olarak
  alıyor.

Sorun (kullanıcı raporu):
1. Aynı bölge için birden fazla motive emit ediliyor (W0 belirsiz) — bunu
   `pick_canonical_per_era` ile kapattık (commit `ef02bd5`).
2. Entry tam dalganın dibine çakıyor — "ezbere" görünüyor; gerçek bir
   trader böyle pozisyon almaz.
3. TP/Entry seviyesi hangi Z'den, hangi formasyondan, hangi dalgadan
   geldiğini söylemiyor (provenance eksik).
4. Yapısal saflık tek başına yetmiyor — itki başladığını söyleyen
   YARDIMCI sinyaller gerek.

---

## I — Yapısal Elliott sinyalleri (Frost & Prechter; Glenn Neely)

### I.1 Tam ABC korreksiyonun bitişi

Elliott'a göre her itkiden sonra korreksiyon gelir; korreksiyonun bitişi
yeni itkinin başlangıcıdır.

| Korreksiyon tipi | Yapı | C-bitişi karakteristik |
|---|---|---|
| **Zigzag** (basit) | 5-3-5 | C ≈ A uzunluğu (equality), 1.272×A veya 1.618×A extension |
| **Flat regular** | 3-3-5 | C ≈ A seviyesi (B ≈ A başlangıcı) |
| **Flat expanded** | 3-3-5 | C, A'nın bitişini 1.382-1.618 oranında geçer |
| **Flat running** | 3-3-5 | C, A bitişine ULAŞAMAZ — B aşırı yükseldi |
| **Zigzag ailesi (W-X-Y)** | iki ABC + X | Y'nin C bacağı 1×A veya 1.618×A |
| **Triangle** (3-3-3-3-3) | A-B-C-D-E | E noktası, daralan/genişleyen sınırlarda apex'e yakın |

**Sinyal**: ZigZag tape'in son N pivotu yukarıdaki kalıplardan birine
oturuyorsa, son pivot **major dip kandidatı**. QTSS bu tarama'yı
zaten yapıyor: `elliott_full` writer (FAZ 25.2 PR'da landed) → `flat_*`,
`combination_wxy_*`, `triangle_*` subkind'ları.

Eksik: bu detection'lar **iq_structures'a feed edilmiyor**. Yani sistem
"W-X-Y bitti, yeni itki başlıyor olabilir" demiyor.

### I.2 Wave 2 → impulse confirmation

Frost-Prechter Rule 1: **Wave 2 NEVER retraces beyond Wave 1 start**.

Pratik anlamı: bir tepe (W1 candidate) sonrası geri çekilme W0'ı kırmadan
durur ve bir yeni tepe yapılırsa, **dalga 2 onaylandı, dalga 3 başlıyor**.
İşte bu, mechanical "W2 anchor entry"den çok daha güçlü bir confirmation.

W2 retracement ortak Fib seviyeleri (en yaygından az yaygına):
- **50.0%** — en sık (Bollinger middle, EMA-21 zone)
- **61.8%** — altın oran, en güvenilir
- **38.2%** — sığ, güçlü trend gösterir
- **78.6%** — derin, ile sınırına yakın
- **88.6%** — invalidasyon yakın

W2 retrace gerçekleştiğinde + price W1 zirvesini KIRDIĞINDA → confirmed.

### I.3 Channel breakout (Daniel-Wright trend channels)

Frost-Prechter §1.2 + Hamilton'ın Dow Theory channel-line tekniği:
- Korreksiyonun A ve C noktalarından geçen trend çizgisi → korreksiyon
  channel'ı
- Bu channel'ın yukarı kırılımı = korreksiyon bitti, yeni itki başlıyor
  sinyali

QTSS şu an bu channel'ı çizmiyor. **`qtss-classical`'ın
`ascending_channel`/`descending_channel` detector'larını ABC pivotlara
uyarlamak gerekir.**

### I.4 Wave 3 = strongest move confirmation

Frost-Prechter Rule 3: **Wave 3 is never the shortest of waves 1, 3, 5**.
Çoğu zaman EN UZUN.

Bir itkide W3 başladıktan sonra, momentum hızlanır. ADX > 25 + RSI > 60
+ MACD bullish cross genelde W3'ün ilk yarısında olur.

---

## II — Teknik analiz (price action + momentum)

### II.1 RSI divergence (klasik bottom signal)

**Bullish Regular Divergence**:
- Price: Lower-Low (yeni dip)
- RSI:   Higher-Low (önceki dipten daha yüksek)
- Anlamı: Satış baskısı azaldı; price yeni dip yapsa da momentum tükendi.
- Güvenilirlik: Daily/Weekly chart'ta yüksek; intraday'de gürültülü.

**Bullish Hidden Divergence** (devam, dönüş değil):
- Price: Higher-Low
- RSI:   Lower-Low
- Anlamı: Pullback bitti, ana trend devam ediyor.

QTSS'in `qtss-indicators` crate'i divergence detector'ını içeriyor (4
çeşit) ama setup pipeline'a feed edilmiyor.

### II.2 MACD signals at major dips

- **Histogram divergence**: price LL, MACD histogram HL → güç birikiyor
- **Bullish cross** (signal line altı → üstü) — oversold bölgede daha
  güvenilir
- **Three-tap pattern**: histogram tepelerinin yükselmesi (artan momentum)

### II.3 Bollinger Band squeeze + breakout

Volatilite kasılması (squeeze) sonrası genişleme yön belirler:
- BB width (top-bot)/mid 30 günlük min seviyesinde → squeeze
- Genişleme yukarı + RSI > 50 → bullish impulse

### II.4 ATR genişlemesi

İtki genelde ATR genişler. Düşük volatilite zone'unda durağan price →
ATR artışı + price upside → trend başlangıcı.

---

## III — Wyckoff method (klasik birikim)

Wyckoff'un major bottom'ları için **kesin event sırası vardır**. Sadece
bir bottom değil — bir **yapı**.

| Faz | Event | Imza |
|---|---|---|
| **A — Stop the trend** | PS (Preliminary Support) | İlk hacim sıçraması, geçici durdurma |
| | SC (Selling Climax) | Panik mum, çok yüksek hacim, alt-shadow uzun |
| | AR (Automatic Rally) | SC sonrası tepki, range tepesini tanımlar |
| | ST (Secondary Test) | SC seviyesine düşük hacimle dönüş, taban onayı |
| **B — Build cause** | Ranging | Düşük hacim, sideway price |
| **C — Test** | Spring | False break below SC, hızlı reclaim |
| | Test of Spring | Düşük hacim ile retest |
| **D — Confirm** | SOS (Sign of Strength) | Range'i yukarı kıran güçlü rally |
| | LPS (Last Point of Support) | SOS sonrası geri çekilme, range üstünde tutma |
| **E — Markup** | BU/JAC (Back-Up) | Range breakout teyidi → trend başlar |

**Major dip = AR sonrası Spring + SOS**. Yani bir tek pivot değil, 4-6
pivot'luk bir yapı.

QTSS'te kısmen var:
- `qtss-wyckoff` crate exists ama production'da çalışmıyor (FAZ 14 backlog)
- `system_config.wyckoff.*` parametreleri seedli (migration 0048)
- Detection table'a yazıcı yok

**Faz 14 dönmesi major dip detection için kritik.**

---

## IV — Volume / Order flow (modern mikro-yapı)

### IV.1 CVD (Cumulative Volume Delta) divergence

Delta = aggressive buy − aggressive sell (her bar). CVD = kümülatif.
- **Price LL + CVD HL** → satıcılar tükendi, smart money birikiyor
- En güvenilir bull bottom signal'lardan biri (Volume Profile community
  consensus)

QTSS'te: CVD `qtss-indicators` içinde implementli. Setup pipeline'a
feed edilmiyor.

### IV.2 Footprint imbalance

Bar içi delta dağılımı:
- Aşağı bar ama bid-side absorption — büyük alıcı duruyor
- Lower wick'te aşırı buy delta → smart money entry

QTSS'te: `qtss-orderflow` crate, FAZ 15'te implement edildi (footprint
imbalance, absorption detector). Production'da çalışıyor.

### IV.3 Liquidation cluster sweep

Coinglass / Binance liquidation heatmap:
- Long liquidation seviyeleri price tarafından silindiğinde, alt-yönlü
  baskı azalır
- Liquidation cluster'ı boşaldığında "max pain" hit edilmiş demek

QTSS'te: `qtss-orderflow::liquidation_stream` Binance `!forceOrder@arr`
stream'ini okuyor (commit görüldü). `liquidation_events` tablosuna
yazıyor.

### IV.4 Funding rate

Perpetual futures funding rate negatife gittiğinde → shorts overcrowded.
- Funding < -0.05% sürekli 7+ gün → bottom signal güçleniyor
- Funding negatiften pozitife flip + price stabil → yön değişiyor

QTSS: `qtss-derivatives-signals::funding_spike` detector (FAZ 12B).

### IV.5 Open Interest dynamics

- OI azalırken price düşüyor → leveraged longs liquidate ediliyor
- OI dip + price dip aynı anda → "clean reset", yeni trend için zemin
- OI artmaya başlarken price stabilize → yeni longs giriyor (potential
  bullish bias)

QTSS: `qtss-derivatives-signals::oi_imbalance` detector.

---

## V — Sentiment & macro

### V.1 Fear & Greed Index

alternative.me F&G index (24h crypto sentiment):
- Score < 25 (Extreme Fear) → tarihsel olarak major bottom'larla
  korelasyon yüksek
- 5-day moving average < 30 → daha güvenilir

QTSS'te: `qtss-fearandgreed` crate var (FAZ 16).

### V.2 BTC Dominance

- Altcoin major dip için BTC.D peak'i izler (altseason ending = alt
  dip de aynı zamana denk gelir)
- BTC için BTC.D rising → BTC absorbs capital → güç sinyali

QTSS: `qtss-macro::btc_dominance` (FAZ 16 plan).

### V.3 Stablecoin supply

USDT/USDC market cap artarken → "dry powder" birikiyor → buying pressure
hazırlığı.

QTSS: yok. Glassnode/Defillama API'si gerek.

---

## VI — Multi-timeframe confluence

### VI.1 Wave-degree mapping (Frost-Prechter §1.4)

Bir TF'deki major dip, BİR ÜST TF'deki Wave 2 veya Wave 4 olabilir:
- 1d Wave 2 dip → 4h chart'ta Wave 2 sub-impulsenin major dip'i
- 1w Wave 4 dip → 1d Wave 4'ün major dip'i

```
1w  Wave 1 ──────── Wave 2 (dip) ──── Wave 3 ...
                       ↓
1d  Sub-1 ─ Sub-2 ─ Sub-3 ─ Sub-4 ─ Sub-5  (sub-impulse forming the W2 dip)
                                    ↓
4h  m1 m2 m3 m4 m5 + ABC down → MAJOR DIP at sub-5 end
```

**Kural**: Bir TF'deki major dip kandidatı, üst TF'in Wave 2/4 retracement
band'ı içine düşüyorsa confidence 2-3× artar.

QTSS: bu mapping yok. iq_structures'ta `parent_timeframe` field var ama
tracker arası feed yapılmamış.

### VI.2 Higher-TF support/resistance

Major dip seviyesi şu seviyelerle örtüşüyorsa güven artar:
- Daily 200 SMA / 50 EMA
- Weekly Pivot Points (S1/S2/S3 — Floor Trader Pivots)
- Monthly Volume Profile VAL/POC
- Anchored VWAP (önceki major top'tan)

QTSS: `qtss-vprofile` aktif (FAZ 6); pivot points yok; anchored VWAP yok.

### VI.3 Multi-TF momentum alignment

- 1h, 4h, 1d hepsinin RSI'i 30 altından kalkış (oversold reset)
- 1h MACD bullish cross ÖNCE, 4h sonra, 1d en geç (cascading)

---

## VII — Crypto-specific signals

### VII.1 On-chain metrics (Glassnode-style)

- **MVRV Z-Score** < 0 (LTH cost basis altında price) → tarihsel bottom
- **Realized Cap Drawdown** %50+ → kapitulasyon
- **Long-term Holder Net Position** azalan → satış tükendi
- **Stablecoin Supply Ratio** (SSR): BTC mcap / Stablecoin mcap → düşük
  SSR = bottom

QTSS'te yok. Glassnode API entegrasyonu gerek.

### VII.2 Exchange flows

- Net BTC outflow from exchanges → birikim (bullish)
- Stablecoin inflow to exchanges → buying power hazırlığı

QTSS'te yok.

### VII.3 Miner capitulation

- Hash ribbons indicator (30-day MA / 60-day MA hash rate cross)
- Miner outflow spike → forced selling
- Cross-up + price stabilization → bottom

QTSS'te yok.

---

## VIII — Composite scoring

Pratik implementasyon: ağırlıklı toplam.

```rust
let major_dip_score: f64 =
      0.20 * structural_completion        // ABC/flat/triangle/wxy bitişi
    + 0.15 * fib_retrace_quality          // 38.2-78.6 zonunda mı
    + 0.15 * volume_capitulation          // SC + ST + Spring
    + 0.10 * cvd_divergence               // Order flow accumulation
    + 0.10 * indicator_alignment          // RSI/MACD div + cross
    + 0.10 * sentiment_extreme            // F&G < 25, social capitulation
    + 0.10 * multi_tf_confluence          // HTF Wave 2/4 + S/R confluence
    + 0.10 * funding_oi_signals           // Funding negatif + OI clean reset
;
```

**Eşikler**:
| Score | Anlam | Aksiyon |
|---|---|---|
| < 0.30 | Düşük güven | Setup açma |
| 0.30 – 0.55 | Gelişen dip | İzle, alarm kur |
| 0.55 – 0.75 | Yüksek güven | Limit emir entry, küçük poz |
| > 0.75 | Çok yüksek | Aggresif entry, full size |

Her sinyal 0..1 normalize. Ağırlıklar GUI-editable (config-driven), her
sembol/TF için override edilebilir.

**Operatör trace edilebilirlik**: setup raw_meta'ya score bileşenleri
yazılır:

```json
{
  "major_dip_score": 0.72,
  "components": {
    "structural_completion": 0.85,  // tam abc_flat_expanded
    "fib_retrace_quality": 0.95,    // 0.618 retrace
    "volume_capitulation": 0.40,    // SC ama ST eksik
    "cvd_divergence": 0.80,         // güçlü divergence
    "indicator_alignment": 0.60,    // RSI div + MACD bullish cross
    "sentiment_extreme": 0.30,      // F&G = 35
    "multi_tf_confluence": 0.90,    // 1d Wave 2 zone
    "funding_oi_signals": 0.70      // funding -0.07%, OI temiz
  },
  "verdict": "high_confidence"
}
```

GUI tooltip'i bu breakdown'ı gösterir → kullanıcı "ezbere değil"
diyebilir.

---

## IX — Provenance fix (TP/Entry hangi Z/dalgadan?)

Kullanıcı: "TP değeri belirlemiş, ancak hangi z için tespit edilen
formasyon veya onun dalgası için net değil."

Mevcut: `qtss_setups.raw_meta` IQ-T için şu alanları taşıyor:

```json
{
  "child_tf": "4h",
  "parent_tf": "1d",
  "parent_slot": 4,
  "iq_structure_id": "...",
  "child_motive_score": 0.97,
  "parent_current_wave": "C"
}
```

GUI'de `IQ_T entry` price line label'ı sadece "IQ_T entry" yazıyor,
detay yok. Fix:

1. Backend `/v2/iq-setups` response'unda mevcut alanlar zaten var.
2. Frontend `priceLineOverlays` çiziminde label'ı zenginleştir:

```typescript
// Eski:
title: "IQ_T entry"
// Yeni:
title: `IQ_T · Z${parent_slot+1} ${parent_current_wave} · ${corrective_kind} · entry`
```

3. Hover tooltip:
```
IQ_T entry
  Source: BTCUSDT 1d Z5 motive
  Wave: parent in C (corrective complete)
  Pattern: flat_expanded
  Score: 0.97 child motive · 0.85 parent
  Detection: abc_forming_bear #def12345
  Major dip score: 0.72 (high)
```

---

## X — Önerilen implementation roadmap

### Faz 25.3.A — Major Dip Score (4 hafta)
- Yeni crate `qtss-major-dip` veya `qtss-analysis` içinde modül
- 8 bileşen: composite scorer, her bileşen pluggable trait impl
- Output: `major_dip_candidates` tablosu (timestamp, symbol, tf, level,
  score, components_json)
- Worker loop her bar kapanışta hesaplar, threshold üzerini yazar

### Faz 25.3.B — Setup gate
- IQ-D / IQ-T candidate loop'lar major_dip_score'ı ÖN ŞART yapar
- score < 0.55 → setup açma
- score >= 0.55 → mevcut entry tier mantığı

### Faz 25.3.C — Confirmation entry trigger
- Setup `armed` durumda iken: entry fiyatı tetiklendiğinde ANINDA
  active'e geçme
- ÖNCE confirmation: bir sonraki bar bullish close (bull setup için),
  RSI > 50, MACD histogram pozitif → o zaman active
- Aksi halde 5 bar bekleme; tetiklenmezse cancel

### Faz 25.3.D — UI provenance + breakdown
- Price line label'ları zenginleştir
- Setup detay paneli: major_dip_score breakdown radar chart
- Chart hover: full provenance tooltip

---

## XI — Bu PR'da ne yok

Yukarıdakilerin **hiçbiri kodlanmadı**. Bu doc sadece araştırma sonucu —
Faz 25.3 sprint'leri olarak ayrı PR'lar halinde gerçekleştirilmesi
önerilir.

Bugünkü teknik fix (commit `ef02bd5`): canonical motive picker — aynı
era'da birden fazla itki yorumunu ONE'a indirir. Major-dip detection
değil, sadece "hangi motive doğru" sorununun bir parçasını çözer.

---

## XII — Concrete component formulas (implementation-ready)

Her bileşen 0..1 normalize skor üretir. Veri kaynakları QTSS'in
mevcut tablolarına / API'lerine bağlandı. Worker loop bunları paralel
hesaplar, weighted-sum'a feeder.

### 12.1 `structural_completion` (w=0.20)

```
input: iq_structures.current_wave + raw_meta.projection.primary_branch
       + raw_meta.projection.branches[primary].score

score:
  if current_wave = "C" AND state = "completed":
      return 1.0
  if current_wave = "C" AND state = "tracking":
      return 0.7 * branches[primary].score
  if current_wave = "B":
      return 0.4 * branches[primary].score
  if current_wave = "A":
      return 0.2
  if current_wave in ("W4", "W5", "W3"):
      return 0.1   // motive in progress, dip not yet
  else:
      return 0.0
```

### 12.2 `fib_retrace_quality` (w=0.15)

```
input: candidate_dip_price, prior_swing_high, prior_swing_low
       (from qtss-pivots L2 swing tape)

retrace_pct = |prior_swing_high − candidate_dip_price|
              / |prior_swing_high − prior_swing_low|

score = peak at canonical Fib levels {0.382, 0.500, 0.618, 0.786},
        linear decay over ±5% buffer:
  for r in [0.382, 0.500, 0.618, 0.786]:
      d = |retrace_pct − r|
      if d <= 0.025:    s = 1.0
      elif d <= 0.075:  s = 1.0 − (d − 0.025) / 0.05
      else:             s = 0.0
      best = max(best, s)
return best
```

### 12.3 `volume_capitulation` (w=0.15)

```
inputs: market_bars window of last 20 bars at candidate dip
        bar.volume, bar.high, bar.low, bar.close, ATR(14)

definitions:
  selling_climax_volume = max(volume) in window
  baseline_volume       = median(volume) excluding the climax bar
  climax_ratio          = selling_climax_volume / baseline_volume
  climax_range_atr      = (high - low) / ATR
  lower_shadow_ratio    = (close - low) / (high - low)

score:
  ratio_score   = clamp((climax_ratio - 1.5) / 1.5, 0..1)   // 1.5x = bare; 3x = full
  range_score   = clamp((climax_range_atr - 1.0) / 1.0, 0..1)
  shadow_score  = clamp(lower_shadow_ratio - 0.5, 0..0.5) * 2.0
return 0.4 * ratio_score + 0.3 * range_score + 0.3 * shadow_score
```

### 12.4 `cvd_divergence` (w=0.10)

```
inputs: cvd series (qtss-indicators::cvd) over last 30 bars
        price low pivots in same window

If two recent pivot lows exist:
  price_low_1, cvd_at_1 (older)
  price_low_2, cvd_at_2 (newer, candidate dip)
  if price_low_2 < price_low_1 AND cvd_at_2 > cvd_at_1:
      // bullish regular divergence
      magnitude = (cvd_at_2 - cvd_at_1) / |cvd_at_1 + 1|
      return clamp(magnitude / 0.5, 0..1)
  else:
      return 0.0
else: return 0.0
```

### 12.5 `indicator_alignment` (w=0.10)

```
inputs: RSI(14), MACD(12,26,9), historical price pivots

components:
  rsi_div      = bullish_divergence(price, rsi) ? 1.0 : 0.0
  macd_cross   = (macd_signal_cross_recent && histogram_positive) ? 1.0 : 0.0
  macd_div     = bullish_divergence(price, macd_histogram) ? 1.0 : 0.0
  rsi_oversold_reset = (rsi was < 30 AND now > 35) ? 1.0 : 0.0

return 0.35 * rsi_div + 0.25 * macd_cross + 0.25 * macd_div
       + 0.15 * rsi_oversold_reset
```

### 12.6 `sentiment_extreme` (w=0.10)

```
inputs: alternative.me Fear & Greed Index (qtss-fearandgreed)

f_and_g_24h = current
f_and_g_5d_avg = mean(last 5 days)

score:
  if f_and_g_24h <= 25:        return 1.0
  if f_and_g_24h <= 35:        return 0.7
  if f_and_g_5d_avg <= 30:     return 0.6   // sustained extreme
  if f_and_g_24h <= 45:        return 0.3
  return 0.0
```

### 12.7 `multi_tf_confluence` (w=0.10)

```
inputs: parent_tf iq_structures row(s) for the same symbol
        (e.g. for 4h candidate: read 1d structure)

definitions:
  parent_struct = iq_structures WHERE timeframe = parent_tf(tf)
                  AND symbol = symbol AND state IN ('candidate','tracking')

score:
  if parent_struct exists:
      if parent_struct.current_wave in ("W2", "W4", "C"):
          // candidate dip aligns with HTF correction zone
          return 1.0
      if parent_struct.current_wave in ("B", "A"):
          return 0.5
      else:
          return 0.2
  else:
      return 0.0
```

### 12.8 `funding_oi_signals` (w=0.10)

```
inputs: derivatives_signals (qtss-derivatives-signals)
        funding_rate_7d_avg, oi_change_24h_pct, price_change_24h_pct

score:
  funding_score = 0.0
  if funding_rate_7d_avg <= -0.0005:    funding_score = 1.0   // -0.05% sustained
  elif funding_rate_7d_avg <= -0.0002:  funding_score = 0.5

  oi_score = 0.0
  if oi_change_24h_pct < -10 AND price_change_24h_pct < -2:
      // OI declining alongside price = liquidation flush, clean reset
      oi_score = 1.0
  elif oi_change_24h_pct < -5:
      oi_score = 0.5

return 0.5 * funding_score + 0.5 * oi_score
```

### 12.9 DB schema — `major_dip_candidates`

```sql
CREATE TABLE major_dip_candidates (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    exchange        TEXT NOT NULL,
    segment         TEXT NOT NULL,
    symbol          TEXT NOT NULL,
    timeframe       TEXT NOT NULL,
    candidate_bar   BIGINT NOT NULL,        -- bar_index of the dip
    candidate_time  TIMESTAMPTZ NOT NULL,
    candidate_price NUMERIC(38,18) NOT NULL,
    score           DOUBLE PRECISION NOT NULL,
    components      JSONB NOT NULL,         -- per-component breakdown
    verdict         TEXT NOT NULL,          -- low/dev/high/very_high
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX major_dip_candidates_recent_idx
    ON major_dip_candidates (symbol, timeframe, candidate_time DESC);
CREATE INDEX major_dip_candidates_score_idx
    ON major_dip_candidates (score DESC, candidate_time DESC);
```

### 12.10 Worker loop topology

```
qtss-worker
  └── major_dip_candidate_loop (NEW, faz 25.3.A)
       ├── tick every 60s
       ├── for each enabled symbol × tf:
       │     1. find candidate_dip = lowest pivot in last N bars
       │        whose age < staleness_horizon
       │     2. for each component above, compute score (0..1)
       │     3. composite = sum(weight * component_score)
       │     4. verdict = bucket(composite) ∈ {low, dev, high, very_high}
       │     5. UPSERT into major_dip_candidates ON (symbol, tf, candidate_bar)
       │     6. if score crossed threshold → pg_notify('major_dip_changed')
       └── consumers:
             ├── iq_d_candidate_loop / iq_t_candidate_loop — gate on score
             └── frontend chart — overlay band at candidate_price
```

### 12.11 Operator config (system_config.major_dip)

```sql
INSERT INTO system_config (module, config_key, value, description) VALUES
  ('major_dip', 'enabled',                '{"enabled": true}'::jsonb,
   'Master switch for major dip candidate worker.'),
  ('major_dip', 'tick_secs',              '{"secs": 60}'::jsonb,
   'How often the major dip worker recomputes scores.'),
  ('major_dip', 'min_score_for_setup',    '{"value": 0.55}'::jsonb,
   'IQ-D/IQ-T won''t spawn setups below this composite score.'),
  ('major_dip', 'weights.structural',     '{"value": 0.20}'::jsonb, ''),
  ('major_dip', 'weights.fib_retrace',    '{"value": 0.15}'::jsonb, ''),
  ('major_dip', 'weights.volume_capit',   '{"value": 0.15}'::jsonb, ''),
  ('major_dip', 'weights.cvd_divergence', '{"value": 0.10}'::jsonb, ''),
  ('major_dip', 'weights.indicator',      '{"value": 0.10}'::jsonb, ''),
  ('major_dip', 'weights.sentiment',      '{"value": 0.10}'::jsonb, ''),
  ('major_dip', 'weights.multi_tf',       '{"value": 0.10}'::jsonb, ''),
  ('major_dip', 'weights.funding_oi',     '{"value": 0.10}'::jsonb, '');
```

### 12.12 Implementation order

1. **25.3.A.1** — migration + `major_dip_candidates` table + config seeds
2. **25.3.A.2** — `qtss-major-dip` crate (or module under qtss-analysis):
   trait `DipScorer`, 8 impls, registry-based dispatch
3. **25.3.A.3** — worker loop `major_dip_candidate_loop`
4. **25.3.A.4** — API endpoint `GET /v2/major-dip/{venue}/{symbol}/{tf}`
5. **25.3.B** — IQ-D/IQ-T gate on `min_score_for_setup`
6. **25.3.C** — confirmation entry trigger (bar close + RSI/MACD post-arm)
7. **25.3.D** — UI overlay + breakdown radar chart

---

## Kaynaklar

1. **Frost & Prechter** — *Elliott Wave Principle: Key to Market Behavior*
   (10th ed). Wave rules + invalidation + Fib relationships.
2. **Glenn Neely** — *Mastering Elliott Wave* (1990). Strict pattern
   identification + corrective complexity.
3. **Robert Prechter** — *The Wave Principle of Human Social Behavior*
   (1999). Sentiment + socionomics.
4. **Richard Wyckoff / Tom Williams** — *Master the Markets* / VSA.
   Volume + accumulation phases.
5. **Larry Williams** — *Long-Term Secrets to Short-Term Trading*. Pivot
   points, futures market microstructure.
6. **John Murphy** — *Technical Analysis of the Financial Markets*.
   Channels, Fib retracements, divergences.
7. **Glassnode** — *Bitcoin Yearly Outlook* reports. MVRV, LTH/STH
   cohort analysis.
8. **CoinDesk Research** — Funding rate / OI bottom-signal studies.
9. **Tradingview public scripts** — community implementations of Wyckoff
   phase detector, CVD divergence, etc.
10. **CME Group** — *Futures Volume Profile Analysis* whitepapers.
