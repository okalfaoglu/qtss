# FAZ 24 — Regime Deep (5-type classifier + detector-aware weighting)

**Status:** placeholder / spec pending
**Predecessors:** Setup v1.2.x, FAZ 22 (IQ Setup), FAZ 23 (Backtest)

## Amaç

Mevcut `qtss-regime` crate altı regime kategorisi üretir (TrendingUp,
TrendingDown, Ranging, Squeeze, Volatile, Uncertain). ChatGPT ve
Gemini bağımsız olarak aynı kör noktayı işaret etti:

- **Aynı "trend" değil:** trend_low_vol ile trend_high_vol
- **Aynı "range" değil:** range_clean ile range_dirty (fake breakout hazineli)
- **news / unstable:** ayrı bir sınıf — makro olayların olduğu dakikalarda
  tüm pattern varsayımları kırılır

FAZ 24: sınıflandırmayı derinleştir + detector ağırlık matrisini yeni
tiplere göre tune et + regime_blackout gate'lerini genişlet.

## Yeni regime tipleri

| Tip | Tespit kriteri |
|---|---|
| `trend_low_vol` | ADX ≥ 25 AND ATR% < 50. yüzdelik | Strong signal, harmonic/motive açılır |
| `trend_high_vol` | ADX ≥ 25 AND ATR% ≥ 50. yüzdelik | Trend var ama SL geniş, breakout/momentum |
| `range_clean` | ADX < 20 AND Choppiness < 50 | Mean-reversion / harmonic PRZ favorisi |
| `range_dirty` | ADX < 20 AND Choppiness ≥ 61.8 | Fake breakout cenneti — tüm trade kapat |
| `squeeze` | BB width < 20. yüzdelik + ATR low | Patlama öncesi, yeni setup açma |
| `news_chaos` | Event blackout window aktif VEYA ATR 5× önceki 50 bar | Hard stop — hiçbir pattern güvenilmez |
| `uncertain` | Yukarıdakilere oturmayan | Default |

## Hurst exponent (Gemini önerisi)

Log-log R/S analysis on last 100 bars:
- `H > 0.6` → persistent trend (trend_* kategorileri güçlenir)
- `H < 0.4` → mean-reverting (range_clean güçlenir)
- `H ≈ 0.5` → random (uncertain/news)

Her `qtss_regime_snapshots` row'una `hurst` kolonu eklenir, confidence
hesabında ağırlık olarak kullanılır.

## Market Profile (Gemini önerisi)

Per-symbol TPO / Volume Profile ile:
- Value Area (VAH/VAL) — range_clean'de mean-reversion hedefi
- Point of Control (POC) — likidite magnet
- Balance zone deviation — range_dirty sinyali

`qtss-vprofile` crate'i zaten var; regime classifier bunu input olarak
tüketecek.

## Detector × Regime matrix

Mevcut `confluence.regime.{family}:{regime}` tablosu 6 × 12 hücre. Yeni
tiplerle 7 × 12 = 84 hücre. Her hücre backtest (FAZ 23) ile tune edilir:

| Family | trend_low_vol | trend_high_vol | range_clean | range_dirty | squeeze | news_chaos |
|---|---|---|---|---|---|---|
| harmonic | 0.6 | 0.4 | **1.4** | 0.3 | 0.5 | 0.0 |
| motive | **1.3** | 1.0 | 0.5 | 0.2 | 0.3 | 0.0 |
| smc | 1.1 | **1.4** | 0.8 | 0.4 | 0.6 | 0.0 |
| classical | 1.0 | 0.8 | 1.0 | 0.6 | 0.7 | 0.0 |
| wyckoff | 1.0 | 0.7 | **1.3** | 0.4 | 0.8 | 0.0 |
| candle | 0.3 | 0.2 | 0.4 | 0.2 | 0.3 | 0.0 |
| ... | | | | | | |

`news_chaos` sütunu tüm 0 — regime bu değerdeyken allocator hiç setup açmaz.

## Alt-fazlar

1. **FAZ 24A** — enum genişletme (domain + regime crate)
2. **FAZ 24B** — Hurst exponent computation + persistence
3. **FAZ 24C** — classifier logic (ADX + ATR percentile + Choppiness + Hurst)
4. **FAZ 24D** — regime_adjuster seed expansion (84 default değer)
5. **FAZ 24E** — `news_chaos` event feed (FOMC/CPI calendar + ATR spike detection)
6. **FAZ 24F** — FAZ 23 backtest entegrasyonu (per-regime Sharpe/DD raporları)

## Bağımlı iş

- FAZ 23 (Backtest) önce — yeni regime weight'lerini doğrulamak için
- FAZ 22 (IQ Setup) önce — macro regime IQ Setup için de input
- FAZ 17 (ML meta-label) sonra — regime'i feature olarak kullanmak için
  kalibre bir train-set gerekli

## Süre tahmini

FAZ 24A-C: 2 hafta · FAZ 24D-E: 1 hafta · FAZ 24F: 2 hafta = ~5 hafta
