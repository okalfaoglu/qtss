# Faz 9 — AI-Yönetimli Birleşik Setup Motoru

> **Durum**: Planlama tamamlandı, uygulama bekliyor (2026-04-16)
> **Önceki faz**: Faz 8 (Commission gate + Wyckoff TTL fix) kapandı
> **Bir sonraki faz**: Faz 10 (Wyckoff Full events) — Faz 9 devreye girince bloksuz ilerler
> **Sahibi**: @okalfaoglu
> **Mimari referans**: `docs/QTSS_V2_ARCHITECTURE_PLAN.md`

## Executive Summary

Faz 9, QTSS'in setup üretim motorunu **tek-kaynak, kural-tabanlı** bir yapıdan **çok-kaynaklı, AI-destekli** bir yapıya dönüştürür. TBM, Wyckoff, Elliott, Classical pattern, Regime Deep ve derivatives verilerini **ortak bir feature vektöründe** toplar, LightGBM meta-model ile setup kalitesini skorlar, belirsiz durumlarda LLM (Claude Haiku prod, Gemini Flash dev) tiebreaker'a gider. Risk katmanı (PositionGuard + Allocator) **değişmez** — AI yalnız "aç/açma" + "boyut" kararına dokunur.

**Sıfır abonelik politikası**: Tüm dış veriler Binance public API + DefiLlama free API ile sağlanır. Nansen/Coinglass/CryptoQuant/Glassnode aboneliği **yok**.

**Dev/Prod LLM ayrımı**: Prod Claude Haiku, dev Gemini Flash veya local Ollama — trait + config ile seçilir.

**Beklenen kazanım**: %15-25 daha yüksek setup hit-rate, yanlış setup'larda %40+ azalma, açıklanabilir kararlar (SHAP).

## Hedefler

1. Her setup kararına **≥5 kaynaktan** katkı
2. Setup üretim kararı **veri-driven** (AI meta-model), ağırlık manuel tuning gerekmez
3. Her karar **açıklanabilir** — operatör "neden açıldı/açılmadı" sorusuna SHAP top-10 feature ile cevap alabilmeli
4. Risk katmanı **değişmez** — AI bozulsa da deterministik veto + PositionGuard koruması
5. Yeni kaynak eklemek **tek trait impl** (CLAUDE.md #1)
6. Tüm eşikler/ağırlıklar **config'ten** (CLAUDE.md #2)

## Non-Hedefler

- RL / deep learning model (Faz 12+)
- Multi-broker execution değişiklikleri (Faz 8C'de)
- Yeni pattern detector eklenmesi — mevcut detector'lar veri kaynağı olarak kullanılacak
- Subscription-based data source entegrasyonu

---

## 1. Bileşen Rol Matrisi

Her kaynağın **güçlü** ve **zayıf** olduğu sorular netleştirilmiştir. AI feature olarak topluyor ama her kaynak yalnız **belirli bir soruda otorite**.

| Kaynak | Görür | Otorite olduğu soru | Zayıf olduğu |
|---|---|---|---|
| **TBM** | Historical setup PnL dağılımı | "Bu setup tarihsel olarak kazanır mı?" | Yeni pattern, regime shift |
| **Wyckoff** | Phase-based accumulation/distribution | "Hangi fazdayız? Spring gerçek mi?" | Trend piyasa |
| **Elliott Wave** | Fraktal dalga + Fibonacci | "Dalga 3'te miyiz, 4'te mi?" | Subjektif sayım |
| **Classical** | Görsel pattern + breakout | "Net kırılım var mı?" | Tamamlanmamış pattern |
| **Regime Deep** | Multi-TF rejim (trend/range/volatile) | "Piyasa bu strateji için uygun mu?" | Rejim geçiş anları |
| **Derivatives** (Binance) | OI, funding, long/short, liquidations | "Squeeze riski? Kalabalık ters mi?" | Yön kendisi değil, bağlam |
| **DefiLlama** | Stablecoin supply, TVL | "Makro likidite akışı?" | Kısa vade (<1d) |
| **Orderbook/CVD** | Mikro akış, absorption | "Entry timing, gerçek baskı" | 1h+ için gürültü |

### Tek-otorite ayrıştırması (çakışan rolleri temizler)

| Karar | Otorite |
|---|---|
| **Yön** | Elliott + Wyckoff + Classical (majority vote, ≥2/3 uyum) |
| **Zamanlama** (trigger) | Classical breakout + CVD + Orderbook |
| **Veto** (aç/açma) | Regime Deep + Derivatives extremes + kill switch |
| **TP hedef** | Elliott Fib + Classical measured move + Liquidation clusters |
| **SL** | Wyckoff structural + Classical invalidation + ATR floor |
| **Boyut** | Regime confidence + TBM historical RR + Allocator (mevcut) |

---

## 2. Mimari — 5 Katmanlı Hibrit

```
┌───────────────────── LAYER 0: Feature Store ────────────────────┐
│ qtss_features_snapshot(detection_id, source, features jsonb)   │
│ Her detection anında 7-9 kaynak feature yazar                  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌───────────────────── LAYER 1: Gate (Rule-Based) ────────────────┐
│ Deterministik veto'lar (AI yok):                               │
│  • Regime strong opposite → reject                             │
│  • News blackout window → reject                               │
│  • Kill switch / margin / correlation cap                      │
│  • Commission gate (Faz 8.1 mevcut)                            │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌───────────────────── LAYER 2: Direction Consensus ──────────────┐
│ Elliott + Wyckoff + Classical majority vote                    │
│ ≥ 2/3 uyum yoksa reject (AI override edemez)                   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌───────────────────── LAYER 3: AI Meta-Model (KALP) ─────────────┐
│ LightGBM 3 model (Rust binding: `lightgbm3` crate):            │
│  • model_p_win: P(TP hit before SL)                            │
│  • model_realized_rr: E[R-multiple] (regression)               │
│  • model_time_to_outcome: E[hold bars] (regression)            │
│ Input: 80-120 feature vektörü (Layer 0'dan)                    │
│ Gate: EV = p_win * E[rr|w] - (1-p_win) * |E[rr|l]| ≥ min_ev    │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌───────────────────── LAYER 4: LLM Tiebreaker (Opsiyonel) ───────┐
│ Sadece belirsiz band (0.45 < p_win < 0.55):                    │
│  • Prod: Claude Haiku 4.5 (~$20-50/ay)                         │
│  • Dev/Test: Gemini 2.0 Flash / Ollama Llama 3.1 8B            │
│ Context + reasoning → JSON verdict                             │
│ Cache: feature_hash → response (hit rate yüksek)               │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌───────────────────── LAYER 5: Risk + Allocator ─────────────────┐
│ Değişmez (mevcut Faz 8.1 sistemi):                             │
│  • PositionGuard (ATR ratchet, struct SL, partial TP)          │
│  • AllocatorLimits (per-venue cap, correlation groups)         │
│  • Commission gate + min_net_rr                                │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                         Open Setup
```

### Neden katmanlı

- **Layer 1-2 deterministik**: AI bozulsa bile saçma setup açmaz (defense in depth)
- **Layer 3 öğrenir**: Feature ağırlıklarını data belirler
- **Layer 4 uç durum**: Açıklanabilirlik + marjinal örneklerde insan-benzeri muhakeme
- **Layer 5 değişmez**: Risk garantisi korunur, AI blast radius sınırlı

---

## 3. Veri Kaynakları — Sıfır Abonelik Politikası

### Kullanılacaklar (hepsi ücretsiz)

| Kaynak | Ne sağlar | Mevcut durum |
|---|---|---|
| **Binance Futures Public API** | OHLCV, OI, funding, long/short, taker volume, premium | ✅ `external_binance_loop` mevcut |
| **Binance WS @forceOrder** | Liquidations stream | ⚠️ Worker yok, Faz 9.0.0'da eklenecek |
| **Binance WS @aggTrade** | Trade stream (CVD için) | ⚠️ Worker yok, Faz 9.0.0'da eklenecek |
| **Binance WS @depth** | Order book | ⚠️ Worker yok, opsiyonel (Faz 9.6) |
| **DefiLlama Public API** | Stablecoin supply, TVL | ⚠️ Worker yok, küçük iş (30dk) |
| **Historical data (QTSS DB)** | TBM, Wyckoff, Elliott, Classical outcomes | ✅ Mevcut |

### Kullanılmayacaklar

| Kaynak | Neden reddedildi |
|---|---|
| ❌ **Nansen** ($150-1800/ay) | QTSS use case (5m-4h BTC/ETH) için overkill. 20k token 12 saatte bitti — model maliyetsiz. Ana pair'lerde marjinal katkı |
| ❌ **Coinglass** ($29/ay) | Veriyi Binance public API'den ücretsiz sağlayabiliyoruz. Sadece aggregation value ekliyor |
| ❌ **CryptoQuant** ($29/ay) | Exchange netflow feature'ı Faz 9 için critical değil; onchain'siz de LightGBM çalışır |
| ❌ **Glassnode** ($39-799/ay) | 1d+ swing için değerli; QTSS ağırlığı 5m-4h |

### Onchain kararı

Onchain feature'ları Faz 9.0'da **opsiyonel** (sparse). LightGBM `missing value` handling native → feature yoksa tolere eder. Gelecekte (Faz 11+) altcoin setup'ları eklenirse onchain revisit edilir.

Mevcut `qtss_v2_onchain_metrics` tablosu (401 satır) kaynağı korunur; nansen_* tabloları **dondurulur** (silinmez, worker disable).

---

## 4. AI Model Spec

### Model seçimi: LightGBM

| Kriter | LightGBM | XGBoost | Neural Net |
|---|---|---|---|
| CPU inference | ~200μs | ~300μs | ~5-50ms |
| Rust binding | `lightgbm3` ✅ | `xgboost` ✅ | ONNX gerekir |
| SHAP | ✅ hızlı | ✅ hızlı | ❌ yavaş |
| Missing values | ✅ native | ✅ native | ❌ manuel |
| Monotonicity constraints | ✅ | ✅ | ❌ |
| Deploy | Binary artifact | Binary artifact | Model server |

**Karar**: LightGBM. Prod'da ~200μs inference CPU-only, training tek makine.

### 3 Ayrı Model (Aynı Feature Vektörü)

| Model | Hedef | Label | Kullanım |
|---|---|---|---|
| `model_p_win` | Binary classification | `outcome ∈ {win, loss}` | Gate kararı |
| `model_realized_rr` | Regression | `realized_rr ∈ ℝ` | EV hesaplama |
| `model_time_to_outcome` | Regression | `hold_bars ∈ ℕ` | Time stop kalibrasyon |

**Karar fonksiyonu**:
```
EV = p_win * E[rr | win] - (1 - p_win) * |E[rr | loss]|
gate_pass = (EV > min_ev_threshold) AND (p_win > min_p_threshold)
```

Tek `p_win` yetersiz — %55 doğruluk ama ortalama -2R kaybeden setup'lar mümkün. RR dağılımı şart.

### LLM Tiebreaker Spec

Sadece `0.45 < p_win < 0.55` ve `|EV| < min_confident_ev` bandında tetiklenir (~%10-15 kararlar).

```rust
#[async_trait]
pub trait LlmJudge {
    async fn evaluate(&self, ctx: &SetupContext) -> LlmVerdict;
    fn name(&self) -> &'static str;
}

pub struct LlmVerdict {
    pub decision: Decision,           // Accept | Reject | Neutral
    pub confidence: f64,              // 0..1
    pub reasoning: String,            // kısa, 2-3 cümle
    pub top_concerns: Vec<String>,    // 0-3 kritik nokta
}

// 3 impl — config.llm.judge.provider ile seçilir:
pub struct ClaudeHaikuJudge { ... }   // prod
pub struct GeminiFlashJudge { ... }   // dev / eval
pub struct OllamaLlamaJudge { ... }   // on-prem / offline
```

Config:
```
llm.judge.enabled               = true
llm.judge.provider              = "claude_haiku"   // gemini_flash | ollama
llm.judge.min_prob_band_low     = 0.45
llm.judge.min_prob_band_high    = 0.55
llm.judge.cache_ttl_secs        = 600
llm.judge.timeout_ms            = 5000
llm.judge.fallback_on_error     = "accept_model_only"  // reject | accept
```

---

## 5. Feature Vektörü — ~100 Kolon

| Grup | Örnek feature'lar | Sayı |
|---|---|---|
| **Wyckoff** | `phase`, `phase_entered_bars_ago`, `reclassify_count`, `confidence`, `range_height_pct`, `creek_break_strength` | ~10 |
| **Elliott** | `current_wave_degree`, `wave_in_progress`, `fib_target_ratio`, `sub_wave_alignment`, `truncation_risk` | ~15 |
| **Classical** | `pattern_type`, `completion_pct`, `height_pct`, `volume_on_breakout`, `measured_move_target` | ~10 |
| **TBM** | `historical_winrate_same_pattern`, `avg_rr_won`, `avg_rr_lost`, `median_hold_bars`, `sample_size` | ~8 |
| **Regime Deep** | `htf_regime`, `ltf_regime`, `regime_agreement`, `atr_percentile_90d`, `hurst_exp`, `adx` | ~12 |
| **Derivatives (Binance)** | `oi_change_24h_pct`, `funding_z_24h`, `long_short_ratio`, `liquidation_24h_z`, `basis_bps`, `taker_buy_sell_ratio` | ~12 |
| **Orderbook/CVD** (Faz 9.6) | `cvd_slope_1h`, `bid_ask_imbalance`, `absorption_score`, `spread_bps` | ~6 |
| **Stablecoin (DefiLlama)** | `stablecoin_supply_change_7d`, `stablecoin_supply_z_30d` | ~4 |
| **Onchain (opsiyonel)** | `exchange_netflow_24h_z` (varsa), `whale_netflow_24h` | ~4 |
| **Session/Time** | `hour_of_day`, `day_of_week`, `is_asia_session`, `is_ny_open`, `minutes_since_regime_change` | ~6 |
| **Volatility** | `atr_14`, `atr_14_percentile_1y`, `bb_width_pct`, `realized_vol_24h` | ~8 |
| **TOPLAM** | | **~95** |

`ConfluenceSource` trait'i her kaynak için feature extractor sağlar:
```rust
#[async_trait]
pub trait ConfluenceSource: Send + Sync {
    fn name(&self) -> &'static str;
    async fn extract_features(&self, ctx: &DetectionContext)
        -> Result<HashMap<String, f64>, SourceError>;
}
```

Registry `static REGISTRY: &[&dyn ConfluenceSource]` (CLAUDE.md #1), yeni kaynak = yeni impl + register satırı.

---

## 6. Veri Altyapısı (Prerequisite)

### 6.1 Outcome Labels

```sql
-- Migration 0113 (planlı)
CREATE TABLE qtss_setup_outcomes (
    setup_id        uuid PRIMARY KEY REFERENCES qtss_v2_setups(id) ON DELETE CASCADE,
    outcome         text NOT NULL CHECK (outcome IN ('win','loss','neutral','invalidated')),
    realized_rr     double precision,
    hold_bars       int,
    close_reason    text,
    closed_at       timestamptz NOT NULL,
    -- reproducibility
    tp_price_used   numeric,
    sl_price_used   numeric,
    entry_price_used numeric,
    close_price_used numeric,
    labeled_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX ix_outcomes_closed ON qtss_setup_outcomes(closed_at);
```

### 6.2 Feature Snapshots

```sql
-- Migration 0114 (planlı)
CREATE TABLE qtss_features_snapshot (
    id                   bigserial PRIMARY KEY,
    detection_id         uuid NOT NULL REFERENCES qtss_v2_detections(id) ON DELETE CASCADE,
    captured_at          timestamptz NOT NULL DEFAULT now(),
    features             jsonb NOT NULL,
    feature_spec_version int NOT NULL,
    UNIQUE (detection_id, feature_spec_version)
);

CREATE INDEX ix_features_captured ON qtss_features_snapshot(captured_at);
CREATE INDEX ix_features_detection ON qtss_features_snapshot(detection_id);
```

### 6.3 ML Predictions

```sql
-- Migration 0115 (planlı)
CREATE TABLE qtss_ml_predictions (
    id              bigserial PRIMARY KEY,
    setup_id        uuid REFERENCES qtss_v2_setups(id) ON DELETE SET NULL,
    detection_id    uuid NOT NULL,
    model_version   text NOT NULL,
    predicted_at    timestamptz NOT NULL DEFAULT now(),
    p_win           double precision,
    expected_rr     double precision,
    expected_hold_bars double precision,
    ev              double precision,
    top_features    jsonb,              -- SHAP top-10
    gate_passed     boolean NOT NULL,
    llm_consulted   boolean NOT NULL DEFAULT false,
    llm_decision    text
);

CREATE INDEX ix_ml_pred_version ON qtss_ml_predictions(model_version, predicted_at);
```

### 6.4 ML Artifacts

```sql
-- Migration 0116 (planlı)
CREATE TABLE qtss_ml_artifacts (
    id              bigserial PRIMARY KEY,
    model_name      text NOT NULL,       -- p_win | realized_rr | time_to_outcome
    version         text NOT NULL UNIQUE,
    trained_at      timestamptz NOT NULL,
    train_start     timestamptz NOT NULL,
    train_end       timestamptz NOT NULL,
    valid_auc       double precision,
    valid_logloss   double precision,
    sample_size     int NOT NULL,
    feature_spec_version int NOT NULL,
    artifact_path   text NOT NULL,       -- local file path or S3 URL
    artifact_sha256 text NOT NULL,
    is_champion     boolean NOT NULL DEFAULT false,
    promoted_at     timestamptz,
    notes           text
);

CREATE INDEX ix_ml_artifacts_champion ON qtss_ml_artifacts(model_name, is_champion)
    WHERE is_champion = true;
```

---

## 7. Training Pipeline

```
┌────────────────────── qtss-ml-train (yeni crate) ─────────────────────┐
│                                                                         │
│ 1. Load:  SELECT features JOIN outcomes                                │
│           WHERE label IS NOT NULL                                       │
│                 AND created_at ∈ [train_start, valid_start)            │
│                                                                         │
│ 2. Walk-forward split:                                                 │
│    - train: son 6 ay (train_start → valid_start)                       │
│    - valid: son 1 ay (valid_start → now)                               │
│    - LEAK-FREE: feature_time ≤ label_setup_created_at ZORUNLU          │
│                                                                         │
│ 3. LightGBM fit (3 model paralel):                                     │
│    - model_p_win       (binary, log_loss)                              │
│    - model_realized_rr (regression, rmse)                              │
│    - model_time_to_outcome (regression, rmse)                          │
│                                                                         │
│ 4. Validation:                                                         │
│    - AUC (p_win)                                                       │
│    - log-loss + Brier score (calibration)                              │
│    - RMSE (realized_rr, time_to_outcome)                               │
│    - Champion'a karşı delta (paired t-test, Diebold-Mariano)           │
│                                                                         │
│ 5. Promotion criteria:                                                 │
│    - AUC(challenger) > AUC(champion) + δ                               │
│    - Brier(challenger) ≤ Brier(champion) * 1.05                        │
│    - PSI(challenger, champion) < 0.25                                  │
│    - Min 200 validation sample                                         │
│                                                                         │
│ 6. Persist: qtss_ml_artifacts + binary artifact                        │
└─────────────────────────────────────────────────────────────────────────┘

Scheduled: ml_train_loop.rs worker, haftalık cron (Pazar 04:00 UTC).
Manual trigger: `cargo run -p qtss-ml-train -- --force`
```

Config:
```
ml.train.enabled                  = true
ml.train.schedule_cron            = "0 4 * * 0"   # Sun 04:00 UTC
ml.train.lookback_months          = 6
ml.train.valid_window_months      = 1
ml.train.min_train_samples        = 500
ml.train.min_valid_samples        = 200
ml.train.auc_promotion_delta      = 0.02
ml.train.psi_max                  = 0.25
```

---

## 8. Inference Pipeline

```rust
// qtss-ml crate
pub struct MlInferenceEngine {
    model_p_win: LightGBMModel,
    model_rr:    LightGBMModel,
    model_time:  LightGBMModel,
    version:     String,
    feature_spec: FeatureSpec,
}

impl MlInferenceEngine {
    pub async fn predict(&self, features: &FeatureSnapshot) -> MlVerdict {
        let x = features.to_vector(&self.feature_spec);
        let p_win = self.model_p_win.predict(&x);
        let e_rr  = self.model_rr.predict(&x);
        let e_t   = self.model_time.predict(&x);
        let shap  = self.model_p_win.shap(&x);  // top-10 feature contributions
        // EV = p*E[rr|w] - (1-p)*|E[rr|l]| (rr modeli her iki dağılımı da içerir)
        let ev = self.expected_value(p_win, e_rr);
        MlVerdict { p_win, expected_rr: e_rr, expected_hold_bars: e_t,
                    ev, top_features: shap, model_version: self.version.clone() }
    }
}
```

**Integration point**: `v2_setup_loop.rs` → `ConfluenceGate::should_open(ctx)` çağrısından sonra, `AllocatorLimits::check()` öncesi.

---

## 9. Risk & Monitoring

### Deterministik Circuit Breakers (AI'dan bağımsız)

```
1. ai.enabled = false              → Layer 3-4 bypass, klasik Layer 1-2 + weighted score
2. Model latency > 100ms (p99)     → auto-fallback
3. Feature coverage < 90%          → auto-fallback
4. PSI > 0.25 (haftalık snapshot)  → alarm + auto-fallback
5. Rolling winrate (son 20 setup) baseline %30 altında → pause + alarm
6. Model age > 30 gün             → alarm (retrain overdue)
```

### Health check endpoint

`GET /healthz/ai` → `{ml_enabled, model_version, model_age_days, last_prediction_ms, psi_current, winrate_30d}`

### Metrics (Prometheus / GUI)

- `qtss_ml_predictions_total{gate_passed, llm_consulted}`
- `qtss_ml_prediction_latency_ms_bucket`
- `qtss_ml_model_auc{model_version}`
- `qtss_ml_feature_coverage_pct`

---

## 10. Yol Haritası — Alt-Fazlar & Görevler

**Takip**: Her alt-faz kapandığında bu dokümanda checkbox işaretlenir, commit mesajı `docs: faz 9.X.Y tamam` referanslar.

### Faz 9.0 — Veri Altyapısı (1.5-2 hafta)

**Amaç**: AI eğitimi için veri toplama altyapısı. Model yok, feature birikmesi başlar.

- [ ] **9.0.0** — Binance derivatives eksiklerini tamamla
  - [ ] Funding rate worker kontrolü (`fapi/v1/fundingRate`) + eksikse snapshot yazımı ekle
  - [ ] `liquidation_stream_loop` — `@forceOrder` WS → `data_snapshots(binance_liquidations_*)`
  - [ ] `aggtrade_cvd_loop` — `@aggTrade` WS → CVD hesapla → `data_snapshots(binance_cvd_*)`
  - [ ] DefiLlama stablecoin worker (opsiyonel, ücretsiz, 30dk)
  - [ ] Nansen worker'ları config flag ile disable (`nansen.enabled = false`)

- [ ] **9.0.1** — Outcome Labeler
  - [ ] Migration 0113: `qtss_setup_outcomes` tablosu
  - [ ] Worker `outcome_labeler_loop.rs`: açık setup → TP/SL hit kontrol → label yaz
  - [ ] Retro-labeling script: mevcut 42 kapalı setup için close_reason → outcome mapping
  - [ ] Config: `outcome.labeler.enabled`, `outcome.labeler.tick_secs`

- [ ] **9.0.2** — Feature Store
  - [ ] Migration 0114: `qtss_features_snapshot` tablosu
  - [ ] `ConfluenceSource` trait'i (qtss-confluence crate'inde veya yeni qtss-features crate'inde)
  - [ ] 7 kaynak feature extractor impl (Wyckoff, Elliott, Classical, TBM, Regime, Derivatives, Session/Vol)
  - [ ] Registry (`inventory` crate veya static `&[&dyn ConfluenceSource]`)
  - [ ] Her detection → feature snapshot yazma hook (orchestrator veya ayrı worker)
  - [ ] GUI: Feature Inspector sayfası (detection_id → feature tablosu)

- [ ] **9.0.3** — Feature spec versioning
  - [ ] `FeatureSpec` struct + `feature_spec_version` integer
  - [ ] Schema değişince bump + migration note

### Faz 9.1 — Klasik Confluence Baseline (1 hafta)

**Amaç**: AI henüz veri toplamıyor, ama setup üretimi klasik çok-kaynaklı olarak devreye alınır. Baseline PnL ölçümü burada başlar.

- [ ] **9.1.1** — Layer 1 (kill switch) + Layer 2 (direction consensus)
  - [ ] `ConfluenceGate::should_open(ctx) -> Result<(), RejectReason>`
  - [ ] Veto registry: `VetoRule` trait + impl'ler (regime_opposite, news_blackout, kill_switch, margin, correlation)
  - [ ] Direction consensus: Elliott+Wyckoff+Classical 2/3 vote

- [ ] **9.1.2** — Layer 3 klasik ağırlıklı skor (ML yerine geçici)
  - [ ] Her kaynak `ConfluenceContribution { score, weight, direction }`
  - [ ] Aggregator: `weighted_sum - contradiction_penalty`
  - [ ] Gate: `final_score ≥ setup.confluence.min_score`
  - [ ] Config: `confluence.weight.{source}`, `confluence.contradiction_penalty`, `confluence.min_score`

- [ ] **9.1.3** — GUI Confluence Inspector
  - [ ] Her setup için: kaynak bazlı contribution radar chart
  - [ ] Veto'lar + neden görünür
  - [ ] Reject'ler de persist ediliyor (`qtss_v2_setup_rejections` mevcut)

- [ ] **9.1.4** — `v2_setup_loop.rs` entegrasyon
  - [ ] ConfluenceGate mevcut allocator öncesi çağrılır
  - [ ] Wyckoff setup loop'u da aynı gate'e girer (Faz 8.2 etkisi)

### Faz 9.2 — Paper Trading + Data Birikimi (4-6 hafta, canlı çalışır)

**Amaç**: Faz 9.1 klasik sistem paper'da çalışırken feature + outcome biriker.

- [ ] Haftalık data sağlık raporu (GUI dashboard veya Telegram)
  - [ ] Feature coverage per source
  - [ ] Outcome label rate
  - [ ] Per (pattern × venue) outcome count
- [ ] **Hedef**: ≥1500 etiketli outcome, her pattern-venue ≥30 örnek
- [ ] Mevcut Wyckoff TTL fix sonrası yapılar tamamlandıkça Wyckoff-only outcome hızlı birikecek

### Faz 9.3 — LightGBM Meta-Model (1 hafta)

**Amaç**: Faz 9.2 veri birikiminin sonunda ilk modelin eğitilmesi.

- [ ] **9.3.1** — `qtss-ml` crate + `lightgbm3` binding
- [ ] **9.3.2** — Training binary `qtss-ml-train`
  - [ ] Feature yüklemesi (DB → DataFrame)
  - [ ] Walk-forward split (leak-free)
  - [ ] 3 model paralel fit
  - [ ] Metrics: AUC, log-loss, Brier, RMSE
  - [ ] Champion/challenger comparison
- [ ] **9.3.3** — Migration 0115 + 0116: `qtss_ml_predictions`, `qtss_ml_artifacts`
- [ ] **9.3.4** — `MlInferenceEngine` (qtss-ml crate'inde)
  - [ ] Model loader + hot reload (SIGHUP)
  - [ ] SHAP top-10 extraction
  - [ ] EV hesaplama
- [ ] **9.3.5** — Shadow mode inference: `v2_setup_loop.rs` klasik karar verir ama ML paralel prediction yazar (`qtss_ml_predictions`)

### Faz 9.4 — AI Shadow → Primary (2 hafta shadow + 1 hafta ramp)

**Amaç**: AI kararları shadow'dan production'a alınır.

- [ ] **9.4.1** — Shadow observation (2 hafta min)
  - [ ] GUI: side-by-side karar karşılaştırması (klasik vs AI)
  - [ ] AI shadow winrate vs klasik baseline
  - [ ] Statistical test: paired binomial
- [ ] **9.4.2** — Health monitoring worker
  - [ ] PSI computation (haftalık)
  - [ ] Calibration plot (haftalık)
  - [ ] Latency p99 tracking
  - [ ] Circuit breaker config
- [ ] **9.4.3** — Promote AI to primary
  - [ ] Config flag: `ai.primary = true`
  - [ ] Klasik fallback her durumda hazır
  - [ ] 1 hafta %10-25-50-100 traffic ramp

### Faz 9.5 — LLM Tiebreaker (0.5 hafta)

**Amaç**: Belirsiz band'da LLM hakem.

- [ ] **9.5.1** — `LlmJudge` trait + 3 impl
  - [ ] `ClaudeHaikuJudge` (prod)
  - [ ] `GeminiFlashJudge` (dev)
  - [ ] `OllamaLlamaJudge` (offline)
- [ ] **9.5.2** — Cache: feature_hash → verdict (in-memory LRU + DB table opsiyonel)
- [ ] **9.5.3** — Prompt template (sistem + user) — versioned, audited
- [ ] **9.5.4** — `v2_setup_loop.rs` entegrasyon: sadece `|p - 0.5| < 0.05` band'ında çağrı
- [ ] **9.5.5** — Config: provider, API key ref, cache TTL, timeout, fallback policy
- [ ] **9.5.6** — Dev vs prod karar uyum testi (%85+ hedef)

### Faz 9.6 — Orderbook / CVD / Liquidation Enrichment (2 hafta, opsiyonel)

**Amaç**: AI baseline çalıştıktan sonra marjinal katkı ölçülerek eklenir.

- [ ] **9.6.1** — Order book worker (`@depth` WS)
- [ ] **9.6.2** — CVD detailed aggregator (1m/5m/15m bucket)
- [ ] **9.6.3** — Liquidation cluster detection (hourly aggregate)
- [ ] **9.6.4** — Yeni feature'lar extractor'a
- [ ] **9.6.5** — Shadow mode karşılaştırma: önceki vs yeni AUC
- [ ] **9.6.6** — Marjinal katkı raporu (SHAP delta analizi)

---

## 11. Başarı Metrikleri

**Faz 9 kabul kriterleri** (9.4 sonrası, canlı 30 gün):

| Metrik | Baseline (Faz 9.1 klasik) | Hedef (Faz 9.4 AI) |
|---|---|---|
| Setup hit rate | TBD (ölçülecek) | +%15-25 |
| False positive rate | TBD | -%40 |
| Avg R-multiple (win) | TBD | ≥1.8R |
| Max drawdown (paper) | TBD | ≤baseline |
| Feature coverage | %90+ | %90+ |
| Model latency p99 | — | <50ms |
| SHAP explanation | Yok | Her setup'ta top-10 |

---

## 12. Açık Sorular

- [ ] **FeatureSpec yönetimi** — `inventory` crate mi, static array mı, SQL tablosundan mı? Öneri: static `&[...]` + compile-time check (CLAUDE.md #1)
- [ ] **GPU training gerekir mi?** — LightGBM CPU yeter; gelecekte NN eklenirse revisit
- [ ] **Model artifact depolama** — Local filesystem (dev) + S3/MinIO (prod)? Şimdilik local yeterli
- [ ] **LLM prompt versioning** — Git vs DB? Öneri: `qtss_llm_prompts` tablosu + audit
- [ ] **Training data retention** — Sınırsız mı (lookback_months ile filtre), yoksa hard delete > 1 yıl? Öneri: sonraki
- [ ] **Multi-symbol eğitim mi single-symbol mu?** — Öneri: başlangıçta multi (data az), sonra per-symbol finetune
- [ ] **Timeframe bazlı ayrı model mi?** — Öneri: TF feature olarak girer, tek model başlangıç

---

## 13. Bağlı Dokümanlar

- `docs/QTSS_V2_ARCHITECTURE_PLAN.md` — Genel mimari
- `docs/FAZ_8C_LIVE_EXECUTION.md` — Live execution spec
- `docs/FAZ_10_WYCKOFF_FULL.md` — Wyckoff full events (Faz 9 sonrası)
- `docs/FAZ_11_REGIME_DEEP.md` — Regime HMM + adaptive (Faz 9 sonrası)
- `docs/CONFIG_REGISTRY.md` — Config key registry

---

## 14. Değişiklik Günlüğü

| Tarih | Değişiklik | Commit |
|---|---|---|
| 2026-04-16 | İlk taslak — AI confluence mimarisi + veri kaynağı kararı (sıfır abonelik) + fazlı yol haritası | (pending) |

---

## 15. Karar Kaydı (Decision Log)

1. **Subscription-free** — Nansen/Coinglass/CryptoQuant/Glassnode abonelik YOK. Binance public API + DefiLlama yeterli. (2026-04-16)
2. **LightGBM over NN** — CPU inference + Rust binding + SHAP + missing-value native handling. (2026-04-16)
3. **3 model, tek karar** — p_win tek başına yetersiz; realized_rr + time_to_outcome ile EV hesaplama. (2026-04-16)
4. **LLM = tiebreaker only** — Belirsiz band'da, primary karar değil. Prod Claude Haiku, dev Gemini. (2026-04-16)
5. **Deterministik Layer 1-2** — AI bozulsa bile veto katmanı koruyor. (2026-04-16)
6. **Shadow → ramp → primary** — Hiçbir AI kararı canlıya direkt girmez. (2026-04-16)
7. **Klasik daima fallback** — `ai.enabled = false` 1 saniyede eski sisteme döner. (2026-04-16)
8. **Wyckoff TTL fix bloker değildir** — Faz 8.2 bonus olarak tamamlandı; Faz 9 onun üzerine. (2026-04-16)
