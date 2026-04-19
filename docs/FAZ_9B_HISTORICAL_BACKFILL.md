# Historical Backfill Spec

**Faz 9B** — Paper trading birikimine alternatif: tarihsel mum verisiyle training set üret.

Canlıda setup biriktirmek yavaş (günde 5-20 setup); eğitime 500+ gerekiyor. Bizde `qtss_bars` tamamen dolu (CCXT historical + live ingest). Detector'lar saf fonksiyon (bar penceresi → Detection). Öyleyse geçmişe doğru **walk-forward replay** yaparsak setup + outcome'ları saatler içinde üretebiliriz.

---

## 1. Neden mümkün?

| Bileşen | Canlıda | Replay'de | Uyumluluk |
|---|---|---|---|
| Bar ingest | exchange WebSocket | `qtss_bars` SELECT | ✓ aynı Bar struct |
| Pivot tree / ATR / SMA | `qtss-pivots` + `qtss-indicators` | aynı crate, aynı input | ✓ saf |
| Detector (candle/classical/wyckoff/elliott/harmonic/pivot) | `SetupGenerator` | aynı API | ✓ side-effect yok |
| Validator + target engine | runtime | aynı | ✓ saf |
| Outcome (TP/SL resolution) | live outcome worker t→∞ takip | **forward-simulate**: `bars[t..t+lookhead]` gezerek TP/SL hangi önce | ✓ deterministic |
| Feature snapshot | feature store writer | aynı computation | ✓ aynı spec_version |

**Tek fark:** `mode='backtest'` kolonu — live setuplarla karışmayı önler.

---

## 2. Algoritma (walk-forward)

```
FOR each symbol IN universe (limit: backfill.symbols_limit):
  FOR each tf IN backfill.timeframes (default H1,H4,D1):
    bars = SELECT * FROM qtss_bars WHERE symbol=? AND tf=? ORDER BY ts
    IF len(bars) < warmup_bars + lookhead_bars: continue

    # Walk-forward cursor. Her t için "sadece t'ye kadar gördüğünü" varsay.
    FOR t IN [warmup_bars .. len(bars) - lookhead_bars]:
      window = bars[0..t+1]                       # no future leak
      indicators = compute_indicators(window)     # SMA, ATR, pivot tree…
      regime = regime_snapshot(window)            # if available

      # Tüm detector'ları çağır — sadece son barda yeni sinyal var mı?
      detections = [
        det.detect(window, indicators)
        for det in ALL_DETECTORS
      ]
      FOR sig IN detections WHERE sig.fired_at_bar == t:
        # Setup validator + target engine (canlıdaki aynı çağrı).
        setup = build_setup(sig, window, indicators, regime)
        IF NOT validator.accept(setup): continue

        # Forward-simulate: bars[t+1 .. t+lookhead_bars]
        outcome = simulate_outcome(setup, bars[t+1 : t+lookhead_bars+1])
        # outcome = {label: win|loss|timeout, r_multiple, closed_at}

        # Feature snapshot (features_by_source) — aynı üretici kod.
        features = feature_store_compute(setup, window, indicators, regime)

        INSERT qtss_setups (..., mode='backtest', created_at=bars[t].ts)
        INSERT qtss_setup_outcomes (setup_id, ..., ...)
        INSERT qtss_features_snapshot (setup_id, features_by_source, ...)
```

**Kritik:** `window = bars[0..t+1]` — t'den sonraki barları detector'a vermek = leakage. Unit test `test_backfill_no_leak.rs` bunu koruyacak.

---

## 3. Config (migration 0169 §E)

| Key | Default | Anlam |
|---|---|---|
| `backfill.enabled` | `false` | Master switch. Operator elle açar, replay biter bitmez false'a çeker. |
| `backfill.warmup_bars` | 500 | Pivot tree + SMA200 için minimum önceki bar. |
| `backfill.lookhead_bars` | 240 | TP/SL resolution için ileri pencere (H1'de 10 gün, D1'de ~8 ay). |
| `backfill.timeframes` | `["H1","H4","D1"]` | Replay edilecek TF'ler. M5/M15 büyük veri yaratır. |
| `backfill.symbols_limit` | 100 | Tek turda max sembol (CPU/disk guard). |
| `backfill.setup_mode_marker` | `"backtest"` | `qtss_setups.mode` kolonuna yazılacak etiket. |

---

## 4. Komut arayüzü

```bash
# Tek sembol, tek TF deneme:
qtss-worker backfill-training --symbols=BTCUSDT --timeframes=H1 --from=2023-01-01

# Tüm universe, migration defaults:
qtss-worker backfill-training

# Yarıda kaldıysa kaldığı yerden:
qtss-worker backfill-training --resume

# İlerleme:
psql -c "SELECT mode, COUNT(*) FROM qtss_setups GROUP BY mode;"
#   live      ↑
#   backtest  ← hızla artar
```

Worker her sembolün sonunda checkpoint yazar (`qtss_ml_backfill_progress` tablosu — Faz 9B ikinci dalga; şu an in-memory):

| symbol | tf | last_ts_processed | setups_emitted | status |

---

## 5. Dikkat edilecekler

### 5.1 Veri kalitesi
- Eski dönemde `derivatives`/`onchain` ingest yoktu → ilgili feature_by_source anahtarları NaN. Coverage gate'i geçmek için en azından `candle`, `classical`, `wyckoff` tam dolu olmalı.
- Bozuk bar (gap, split, rename) → warmup öncesi filtrelenmeli.

### 5.2 Regime snapshot
Regime üretimi canlı akışa bağlı (ATR persist, MACD slope). Replay'de aynı formülü bar penceresi üzerinde recompute etmek zorunda. `qtss-regime` crate bu iki moda da servis verecek (refactor gereği).

### 5.3 Outcome realism
- **Slippage**: canlıdaki execution adapter spread/slippage uyguluyordu. Replay'de `mid + ATR * k` şeklinde pesimistik fill modeli kullan (config: `backfill.slippage_atr_mult`).
- **Kommisyon**: `commission gate` (Master gap list'teki item) replay'de de aktif olsun. Net kâr < komisyon → setup skip.

### 5.4 Feature spec version
Replay anındaki feature üreticisi **şu anki kod**. Eski dönemler için "o zaman hangi feature vardı?" sorusu anlamsız — biz şu an kullanabildiğimiz her feature'ı geriye doğru recompute ediyoruz. Bu normal; training set bu yüzden "sanki bugün başlasaydı nasıl görürdü"yü simüle eder.

### 5.5 Out-of-distribution riski
Tarihsel piyasa + live piyasa farklı rejim olabilir (ör. 2022 bear, 2024 bull). Training set `mode='backtest'` + `mode='live'` karışık olunca model her iki rejimi de görüyor, bu iyi. Ama **sadece** backtest ile eğitirsen canlı dağılımla farklılık riski var — deployment checklist Aşama 2 bu konuyu not eder.

---

## 6. Beklenen üretim

Kaba tahmin (1 sembol, H1, 2 yıllık geçmiş = ~17000 bar):

- Detector çağrısı: ~17000 × 6 crate = 100k çağrı
- Setup emit oranı: ~%0.3-0.8 → 50-140 setup
- 100 sembol × 3 TF → **15000-40000 training example**

Min_rows=500 eşiğini kolayca geçer; coverage gate + balance gate zayıf feature'ları eler.

**Süre:** Ryzen 7 / WSL2, 100 sembol × 3 TF ≈ **4-8 saat** (paralelizasyonla).

---

## 7. Aktivasyon politikası — backfill modeli için

Backfill'den üretilen model `auto_activate_if_better=true` koşulunda canlıya geçer **ama** ilk 24-48 saat shadow kalması önerilir (deployment checklist Aşama 5). Gerçekçi live AUC ölçülmeden production decision vermek riskli.

---

## 8. İlişkili dokümanlar

- [Retraining playbook](FAZ_9B_RETRAINING_PLAYBOOK.md) — T6 trigger: backfill bittikten sonra retrain
- [Drift runbook](FAZ_9B_DRIFT_RUNBOOK.md) — "sembol evreni değişti" maddesi backfill'i tetikler
- [Deployment checklist](FAZ_9B_MODEL_DEPLOYMENT_CHECKLIST.md) — Aşama 2 mode kırılımı kontrolü
