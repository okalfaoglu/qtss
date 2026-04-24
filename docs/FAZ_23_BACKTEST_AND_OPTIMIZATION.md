# FAZ 23 — Backtest & Optimization Engine

**Status:** placeholder / spec pending
**Predecessors:** Setup v1.1.x (fully gated live-paper pipeline),
                   FAZ 20 (RADAR), FAZ 22 (IQ Setup)

## Amaç

Mevcut sistem `live paper` modunda çalışır — gerçek fiyatla, gerçek
zamanda, simüle broker emirleriyle. Bu güzel **forward test**'tir, ama:

- Yeni parametre kombinasyonu denemek için **saatler/günler** beklenir
- Her config ayarında eski örnekler silinir, tarihsel kıyaslama yok
- Parametreler kör tune ediliyor (önsezi + canlı gözlem)
- Çok detector × çok sembol × çok TF = arama uzayı manuel taranamaz

**FAZ 23**: `market_bars` tablosundaki **geçmiş veriler üzerinde** tüm
detector → confluence → allocator → execution zincirini yeniden
çalıştıran bir motor. Parametre optimizasyon grid'i + walk-forward
validation + bias-aware metrik raporları.

## Kapsam

### 1. Backtest engine (`qtss-backtest` crate zaten var, genişletilecek)

Girdi:
- `exchange, symbol, timeframe` listesi
- Tarih aralığı (örn. 2025-01-01 → 2025-12-31)
- Config snapshot (tüm `system_config` parametreleri JSON)
- Başlangıç sermayesi + risk_pct

Akış:
1. `market_bars` sıralı oku
2. Her bar'da: **eş-zamanlı simüle** pivots → detections → confluence
   → allocator → setup → live_position
3. Tick fill yok (daha hızlı): bar close/open/high/low ile SL/TP kontrolü
4. Tüm kapanan trade'ler yapay `backtest_runs.trades` tablosuna yazılır

Çıktı:
- Win rate, PnL, max drawdown, Sharpe, Calmar
- Per-profile, per-symbol, per-direction breakdown
- Equity curve JSON

### 2. Optimizasyon modülü (yeni)

**Grid search** (başlangıç):
```
parameters_grid:
  atr_sl_mult: [0.8, 1.0, 1.2]
  atr_tp_mult_0: [1.5, 2.0, 2.5, 3.0]
  min_abs_net_score: [1.0, 1.5, 2.0]
  commission_safety_multiple: [1.5, 2.0, 2.5]
  time_stop.max_bars_d: [16, 24, 36]
  corr_cluster_max_armed: [1, 2, 3]
```

Tüm kombinasyonlar çalıştırılır (3×4×3×3×3×3 = 972 run). Her run:
- Backtest engine çalıştırılır
- Sharpe / Calmar / max DD / win rate kaydedilir
- Sonuçlar `optimization_runs` tablosu + GUI Pareto plot

**Walk-forward validation** (overfitting koruması):
- Train window: 90 gün
- Test window: 30 gün
- Sliding: 7 gün
- Her pencerede ayrı optimizasyon → "in-sample" parametreleri
  "out-of-sample" test et
- Parameter robustluk skoru = OOS performance / IS performance

**Bayesian optimization** (gelişmiş, optional):
- Grid yerine intelligent search
- Gaussian Process surrogate model
- Her 20 iteration'da en iyi 10'u şortla

### 3. GUI entegrasyonu

Yeni sayfa: `/v2/backtest`
- Backtest run oluştur formu (config JSON + date range)
- Çalışan run'ların progress bar'ı
- Tamamlanan run'lar listesi + equity curve
- Pareto plot (Sharpe vs Max DD)
- Top 10 parametre setini "apply to live" butonu

### 4. Bias-aware metrikler

Yaygın backtest tuzakları (hepsi raporlanacak):
- **Look-ahead bias**: detector `raw_meta`'ya gelecek veri sızmış mı
- **Survivorship bias**: delisted coin'ler dahil mi (şu an binance-only)
- **Fee/slippage realism**: `execution.dry.slippage_bps` honored mı
- **Order book depth**: büyük emir fill edilebilir mi (pozisyon boyutu
  bar hacminin %1'inden az olmalı)
- **Funding cost**: uzun-süreli pozisyonlar funding yer, hesaba katılmalı
- **Spread widening**: volatil anlarda spread artar, slippage artar

### 5. Tablolar (migration 0250+)

```sql
CREATE TABLE backtest_runs (
    id            UUID PRIMARY KEY,
    config        JSONB NOT NULL,   -- snapshot of system_config
    start_date    DATE NOT NULL,
    end_date      DATE NOT NULL,
    starting_capital NUMERIC NOT NULL,
    -- results
    closed_trades INT,
    win_rate      NUMERIC,
    total_pnl     NUMERIC,
    max_drawdown  NUMERIC,
    sharpe        NUMERIC,
    calmar        NUMERIC,
    equity_curve  JSONB,
    created_at    TIMESTAMPTZ DEFAULT now(),
    finished_at   TIMESTAMPTZ
);

CREATE TABLE optimization_runs (
    id           UUID PRIMARY KEY,
    grid         JSONB NOT NULL,   -- parameter grid
    best_run_id  UUID REFERENCES backtest_runs(id),
    pareto       JSONB,            -- Pareto-optimal set
    created_at   TIMESTAMPTZ DEFAULT now(),
    finished_at  TIMESTAMPTZ
);
```

## Alt-fazlar

1. **FAZ 23A** — Backtest engine iskelet: bar replay + detector invocation
2. **FAZ 23B** — Setup simülasyonu: allocator + execution_bridge test uyumu
3. **FAZ 23C** — Metrik + rapor: Sharpe/Calmar/DD hesapları
4. **FAZ 23D** — Grid search + Pareto plot
5. **FAZ 23E** — Walk-forward validation
6. **FAZ 23F** — GUI sayfası + "apply to live" workflow

## Open questions

- Bar replay sırası: her bar'da tüm engine writers yeniden mi çalışır,
  yoksa detection'lar önceden hesaplanıp cache mi ediliyor?
- Tick-level backtest (her bar'ın tick dağılımı) veya bar-level yeterli
  mi? Tick daha gerçekçi ama 1000× daha yavaş.
- Forward test sonuçları backtest sonuçlarıyla otomatik karşılaştırılsın
  mı (live divergence detection)?
- Multi-symbol portfolio backtest (sermaye paylaşımı, correlation) Faz
  23'e mi yoksa ayrı Faz 24'e mi bırakılsın?

## İlk milestone hedefi

Faz 23A: Bir sembol × bir TF × 30 gün için, mevcut config ile backtest
çalıştırılıyor. Sonuç: win rate + total PnL + equity curve JSON. Manual
validation: aynı dönemin live paper sonuçlarıyla yakın olmalı (±%15
tolerance — perfect match imkansız çünkü bar-level vs tick-level fark).

---

**Süre tahmini:** 23A-23C ≈ 2 hafta, 23D-23E ≈ 2 hafta, 23F ≈ 1 hafta.
Toplam 5 hafta iş.
