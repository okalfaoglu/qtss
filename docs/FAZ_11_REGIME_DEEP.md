# Faz 11 — Regime Deep Integration

> Status: Planned · 2026-04-11
> Bağımlılık: Faz 1 (mevcut regime crate), Faz 3 (validator alignment), Faz 10 (Wyckoff state machine)

---

## 1. Hedef

Regime sistemini **yüzeysel bir HUD**'dan **karar verici bir katmana** yükseltmek:
- Multi-timeframe regime confluence
- Regime geçiş (transition) tespiti ve erken uyarı
- Hidden Markov Model (HMM) ile istatistiksel rejim tahmini
- Strateji/detector parametrelerinin rejime göre **adaptif** ayarlanması
- GUI'de rejim haritası, geçiş timeline'ı ve performans korelasyonu

---

## 2. Mevcut Durum

| Bileşen | Durum |
|---------|-------|
| `qtss-regime` crate | 6 rejim: TrendingUp/Down, Ranging, Squeeze, Volatile, Uncertain |
| Hesaplama | ADX, BB width, ATR/price ratio, Choppiness Index — kural tabanlı |
| DB depolama | `qtss_v2_detections.regime` JSONB kolonu (detection bazlı) |
| Validator kullanımı | RegimeAlignment: pattern ailesi → tercih edilen rejim eşlemesi (+0.3/-0.2 bonus) |
| GUI | `/v2/regime` sayfası: tek sembol, renk kodlu pill, 4 gösterge, 60-nokta geçmiş |
| Multi-timeframe | Yok |
| Geçiş tespiti | Yok |
| HMM / ML | Yok |
| Adaptif parametreler | Yok |

---

## 3. Mimari Tasarım

### 3A. Multi-Timeframe Regime Confluence

Aynı sembol için farklı timeframe'lerde rejim analizi yapılır ve **confluence skoru** hesaplanır:

```
                 1h         4h         1d
             ┌─────────┬──────────┬──────────┐
TrendingUp   │  ✅ 0.7  │  ✅ 0.8  │  ✅ 0.9  │  → Strong Trend Confluence (0.80)
Ranging      │  ❌       │  ❌       │  ❌       │
Squeeze      │  ❌       │  ❌       │  ❌       │
             └─────────┴──────────┴──────────┘

                 1h         4h         1d
             ┌─────────┬──────────┬──────────┐
TrendingUp   │  ✅ 0.6  │  ❌       │  ❌       │  → Mixed / Transition (0.35)
Ranging      │  ❌       │  ✅ 0.5  │  ❌       │
TrendingDown │  ❌       │  ❌       │  ✅ 0.7  │
             └─────────┴──────────┴──────────┘
```

```rust
struct MultiTfRegime {
    symbol: String,
    snapshots: Vec<(String, RegimeSnapshot)>,  // (interval, snapshot)
    dominant_regime: RegimeKind,
    confluence_score: f64,                      // 0.0–1.0
    is_transitioning: bool,
}
```

**Timeframe ağırlıkları** (config):
- `regime.tf_weights`: `{"5m": 0.1, "15m": 0.15, "1h": 0.25, "4h": 0.30, "1d": 0.20}`

### 3B. Regime Transition Detection

Rejim **geçişleri** genellikle en karlı fırsatları barındırır:
- Squeeze → TrendingUp/Down (breakout)
- Ranging → Volatile (expansion)
- TrendingUp → Ranging (trend sonu, muhtemel reversal)

```rust
struct RegimeTransition {
    symbol: String,
    interval: String,
    from_regime: RegimeKind,
    to_regime: RegimeKind,
    transition_speed: f64,        // 0-1, ne kadar hızlı geçiş
    confirming_indicators: Vec<String>,  // hangi göstergeler teyit ediyor
    detected_at: DateTime<Utc>,
    confidence: f64,
}
```

**Geçiş kuralları** (config tablosundan):

| Geçiş | Tetikleyici | Sinyal Anlamı |
|--------|------------|---------------|
| Squeeze → TrendingUp | BB genişleme + ADX > 25 + +DI > -DI | Bullish breakout |
| Squeeze → TrendingDown | BB genişleme + ADX > 25 + -DI > +DI | Bearish breakout |
| TrendingUp → Ranging | ADX düşüş + Choppiness artış | Trend yavaşlama, range beklentisi |
| TrendingUp → TrendingDown | +DI/-DI crossover + güçlü momentum | Trend reversal |
| Ranging → Volatile | ATR spike + BB genişleme | Range kırılma |

### 3C. Hidden Markov Model (HMM)

Kural tabanlı sınıflandırmaya **ek** olarak istatistiksel model:

**Hidden States**: 4 (trend_bull, trend_bear, range, volatile)
**Observable**: ADX, BB_width, ATR_pct, Choppiness, Volume_ratio
**Training**: Son 1000 bar üzerinde Baum-Welch algoritması

```rust
struct HmmRegimeModel {
    n_states: usize,              // 4
    transition_matrix: Vec<Vec<f64>>,  // 4x4
    emission_params: Vec<GaussianMixture>,
    initial_probs: Vec<f64>,
    state_labels: Vec<RegimeKind>,
}

impl HmmRegimeModel {
    fn predict(&self, observations: &[ObservationVector]) -> Vec<(RegimeKind, f64)>;
    fn transition_probability(&self, from: RegimeKind, to: RegimeKind) -> f64;
    fn expected_duration(&self, state: RegimeKind) -> f64;  // bars
}
```

**Avantaj**: HMM her rejimin **beklenen süresini** ve **geçiş olasılığını** verebilir → daha güvenilir forward-looking sinyal.

**Kütüphane seçenekleri:**
- Pure Rust: `hmmlearn` port veya `ndarray` ile manual impl
- Config: `regime.hmm_enabled` (default: false), `regime.hmm_train_bars` (default: 1000)

### 3D. Adaptif Parametre Sistemi

Rejime göre detector/validator/strategy parametrelerini otomatik ayarlama:

```rust
struct AdaptiveParam {
    base_value: f64,
    regime_overrides: HashMap<RegimeKind, f64>,
}
```

| Parametre | TrendingUp | TrendingDown | Ranging | Squeeze | Volatile |
|-----------|-----------|-------------|---------|---------|----------|
| `elliott.min_confidence` | 0.55 | 0.55 | 0.70 | 0.80 | 0.65 |
| `harmonic.min_confidence` | 0.70 | 0.70 | 0.50 | 0.60 | 0.65 |
| `risk.max_position_pct` | 3.0% | 3.0% | 2.0% | 1.5% | 1.0% |
| `strategy.profit_target_mult` | 3.0 | 3.0 | 1.5 | 2.0 | 1.2 |
| `strategy.stop_loss_mult` | 1.5 | 1.5 | 1.0 | 0.8 | 2.0 |
| `wyckoff.range_edge_tolerance` | — | — | 0.02 | 0.015 | 0.04 |

**Uygulama:**
```sql
-- system_config'e ek kolon veya ayrı tablo
CREATE TABLE IF NOT EXISTS regime_param_overrides (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    module      TEXT NOT NULL,
    config_key  TEXT NOT NULL,
    regime      TEXT NOT NULL,  -- trending_up, trending_down, ranging, squeeze, volatile
    value       JSONB NOT NULL,
    created_at  TIMESTAMPTZ DEFAULT now(),
    UNIQUE(module, config_key, regime)
);
```

### 3E. Regime-Based Strategy Selection

```rust
trait RegimeAwareStrategy {
    fn preferred_regimes(&self) -> Vec<RegimeKind>;
    fn regime_weight(&self, regime: &RegimeSnapshot) -> f64;
}
```

| Strateji Ailesi | Tercih Edilen Rejimler | Kaçınılacak |
|----------------|----------------------|-------------|
| Elliott Wave | TrendingUp, TrendingDown | Ranging, Squeeze |
| Harmonic | Ranging, Volatile | Strong Trending |
| Classical (H&S, etc.) | Transition anları | Squeeze |
| Wyckoff | Ranging → Transition | Strong Trending |
| Range Bound | Ranging | TrendingUp/Down |
| Breakout | Squeeze → Trending geçişi | Ranging |

---

## 4. GUI Tasarımı

### 4A. Regime Dashboard (`/v2/regime` — genişletilmiş)

```
┌─────────────────────────────────────────────────────────────────────┐
│  REGIME DASHBOARD                                           [⚙️]   │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌── BTCUSDT ────────────────────────────────────────────────────┐  │
│  │  1h: 🟢 TrendingUp (0.72)  │  4h: 🟢 TrendingUp (0.81)     │  │
│  │  1d: 🔵 Ranging (0.54)     │  Confluence: 0.69 (Moderate)    │  │
│  │  Dominant: TrendingUp       │  Transitioning: ⚠️ Possible     │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                     │
│  ┌── ETHUSDT ────────────────────────────────────────────────────┐  │
│  │  1h: 🟡 Squeeze (0.68)     │  4h: 🟡 Squeeze (0.75)        │  │
│  │  1d: 🔵 Ranging (0.61)     │  Confluence: 0.72 (Strong)     │  │
│  │  Dominant: Squeeze          │  Transitioning: 🔴 Imminent    │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                     │
│  ┌── Regime Transition Alerts ───────────────────────────────────┐  │
│  │  🔔 ETHUSDT: Squeeze → TrendingUp likely (HMM: 72%)         │  │
│  │  🔔 SOLUSDT: TrendingUp → Ranging (ADX declining)           │  │
│  │  🔔 LINKUSDT: Ranging → Volatile (ATR expanding)            │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                     │
├─────────────────────────────────────────────────────────────────────┤
│  Regime Heatmap (tüm semboller × timeframe'ler)                    │
│                                                                     │
│  Symbol     │ 5m  │ 15m │  1h │  4h │  1d │ Dominant │ Score │     │
│  ──────────┼─────┼─────┼─────┼─────┼─────┼──────────┼───────│     │
│  BTCUSDT   │ 🟢  │ 🟢  │ 🟢  │ 🟢  │ 🔵  │ TrendUp  │ 0.69  │     │
│  ETHUSDT   │ 🟡  │ 🟡  │ 🟡  │ 🟡  │ 🔵  │ Squeeze  │ 0.72  │     │
│  SOLUSDT   │ 🔴  │ 🔴  │ 🟢  │ 🟢  │ 🟢  │ Mixed    │ 0.41  │     │
│  BNBUSDT   │ 🔵  │ 🔵  │ 🔵  │ 🔵  │ 🔵  │ Ranging  │ 0.83  │     │
│  LINKUSDT  │ 🟣  │ 🟣  │ 🔵  │ 🟢  │ 🟢  │ Transit  │ 0.55  │     │
│                                                                     │
│  🟢 TrendUp  🔴 TrendDown  🔵 Ranging  🟡 Squeeze                  │
│  🟣 Volatile  ⚪ Uncertain                                          │
└─────────────────────────────────────────────────────────────────────┘
```

### 4B. Regime Timeline (Chart Overlay)

Chart altında **renkli bant** olarak rejim geçmişi:
```
│ bar bar bar bar bar bar bar bar bar bar bar bar bar bar │
│████████████████░░░░░░░░░░████████████░░░░░░░░██████████│
│   TrendingUp   │ Ranging │ Squeeze  │Ranging│TrendUp  │
│                                    ↑ TRANSITION ALERT  │
```

### 4C. Regime Performance Correlation

Her rejimde stratejilerin performansı:
```
Regime Performance (son 30 gün)
─────────────────────────────────────────
              │ Win Rate │ Avg P&L │ Count
TrendingUp    │   68%    │ +1.2%   │  42
TrendingDown  │   61%    │ +0.8%   │  18
Ranging       │   55%    │ +0.3%   │  31
Squeeze       │   72%    │ +2.1%   │   8
Volatile      │   38%    │ -0.5%   │  15
```

### 4D. Regime → Strategy Recommendation Panel

```
Mevcut Rejim: Squeeze → TrendingUp geçiş ihtimali yüksek

Önerilen Stratejiler:
  ✅ Breakout Long (Squeeze→TrendUp sinyali)
  ✅ Elliott Impulse (trend başlangıcı)
  ⚠️ Harmonic (ranging biterken riskli)
  ❌ Range Bound (squeeze kırılıyor)

Risk Ayarı: Position size %50 artırılabilir (HMM confidence: 0.78)
```

---

## 5. DB Tabloları

### 5A. Regime Snapshots (ayrık tablo)

```sql
-- 0052_regime_snapshots_table.sql
CREATE TABLE IF NOT EXISTS regime_snapshots (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol        TEXT NOT NULL,
    interval      TEXT NOT NULL,
    regime        TEXT NOT NULL,           -- trending_up, trending_down, ranging, squeeze, volatile, uncertain
    trend_strength TEXT,                    -- weak, moderate, strong, very_strong
    confidence    DOUBLE PRECISION NOT NULL,
    adx           DOUBLE PRECISION,
    plus_di       DOUBLE PRECISION,
    minus_di      DOUBLE PRECISION,
    bb_width      DOUBLE PRECISION,
    atr_pct       DOUBLE PRECISION,
    choppiness    DOUBLE PRECISION,
    hmm_state     TEXT,                    -- HMM predicted state (nullable)
    hmm_confidence DOUBLE PRECISION,       -- HMM confidence
    computed_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_regime_snapshots_lookup ON regime_snapshots (symbol, interval, computed_at DESC);

-- Retention: keep last 7 days
-- Partition by computed_at monthly if volume warrants
```

### 5B. Regime Transitions

```sql
-- 0053_regime_transitions_table.sql
CREATE TABLE IF NOT EXISTS regime_transitions (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol            TEXT NOT NULL,
    interval          TEXT NOT NULL,
    from_regime       TEXT NOT NULL,
    to_regime         TEXT NOT NULL,
    transition_speed  DOUBLE PRECISION,
    confidence        DOUBLE PRECISION NOT NULL,
    confirming_indicators JSONB DEFAULT '[]',
    hmm_probability   DOUBLE PRECISION,
    detected_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at       TIMESTAMPTZ,          -- geçiş teyit edildiğinde
    was_correct       BOOLEAN               -- backtest doğrulaması
);

CREATE INDEX idx_regime_transitions_active ON regime_transitions (symbol, interval)
    WHERE resolved_at IS NULL;
```

### 5C. Regime Parameter Overrides

```sql
-- 0054_regime_param_overrides.sql
CREATE TABLE IF NOT EXISTS regime_param_overrides (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    module      TEXT NOT NULL,
    config_key  TEXT NOT NULL,
    regime      TEXT NOT NULL,
    value       JSONB NOT NULL,
    description TEXT,
    created_at  TIMESTAMPTZ DEFAULT now(),
    updated_at  TIMESTAMPTZ DEFAULT now(),
    UNIQUE(module, config_key, regime)
);

-- Seed default overrides
INSERT INTO regime_param_overrides (module, config_key, regime, value, description) VALUES
  ('risk', 'max_position_pct', 'volatile',    '1.0',  'Volatile rejimde pozisyon boyutu %1'),
  ('risk', 'max_position_pct', 'squeeze',     '1.5',  'Squeeze rejimde pozisyon boyutu %1.5'),
  ('risk', 'max_position_pct', 'trending_up', '3.0',  'Trend rejimde pozisyon boyutu %3'),
  ('risk', 'max_position_pct', 'ranging',     '2.0',  'Range rejimde pozisyon boyutu %2'),
  ('strategy', 'profit_target_mult', 'trending_up',  '3.0', 'Trend: geniş hedef'),
  ('strategy', 'profit_target_mult', 'ranging',      '1.5', 'Range: dar hedef'),
  ('strategy', 'stop_loss_mult',     'volatile',     '2.0', 'Volatile: geniş stop'),
  ('strategy', 'stop_loss_mult',     'squeeze',      '0.8', 'Squeeze: dar stop')
ON CONFLICT (module, config_key, regime) DO NOTHING;
```

---

## 6. Config Seed

```sql
-- 0055_regime_deep_config.sql
INSERT INTO system_config (module, config_key, value, description) VALUES
  -- Multi-TF
  ('regime', 'multi_tf_enabled',        'true',     'Multi-timeframe regime confluence'),
  ('regime', 'tf_weights',              '{"5m":0.1,"15m":0.15,"1h":0.25,"4h":0.30,"1d":0.20}', 'TF weights'),
  ('regime', 'timeframes',             '["15m","1h","4h","1d"]', 'Timeframes to compute regime on'),
  -- Transitions
  ('regime', 'transition_detection_enabled', 'true', 'Detect regime transitions'),
  ('regime', 'transition_min_confidence',    '0.6',  'Min confidence for transition alert'),
  -- HMM
  ('regime', 'hmm_enabled',            'false',    'Hidden Markov Model regime prediction'),
  ('regime', 'hmm_train_bars',         '1000',     'Bars to train HMM on'),
  ('regime', 'hmm_retrain_interval_h', '24',       'Retrain HMM every N hours'),
  -- Adaptive
  ('regime', 'adaptive_params_enabled', 'true',    'Auto-adjust detector params per regime'),
  -- Snapshot
  ('regime', 'snapshot_tick_secs',      '300',     'Regime snapshot computation interval'),
  ('regime', 'snapshot_retention_days', '7',       'Days to keep regime snapshots')
ON CONFLICT (module, config_key) DO NOTHING;
```

---

## 7. API Endpoint'leri

| Method | Path | Açıklama |
|--------|------|----------|
| GET | `/v2/regime/dashboard` | Tüm semboller, multi-TF regime confluence |
| GET | `/v2/regime/heatmap` | Sembol × TF heatmap verisi |
| GET | `/v2/regime/{symbol}` | Tek sembol, tüm TF'ler, geçiş bilgisi |
| GET | `/v2/regime/transitions` | Aktif ve son geçişler |
| GET | `/v2/regime/performance` | Rejim bazlı strateji performansı |
| GET | `/v2/regime/params/{regime}` | Rejim-özel parametre override'ları |
| PUT | `/v2/regime/params/{regime}` | Override güncelle |
| GET | `/v2/regime/timeline/{symbol}/{interval}` | Chart overlay için rejim timeline |
| GET | `/v2/regime/hmm/{symbol}/{interval}` | HMM tahmin ve geçiş olasılıkları |

---

## 8. Worker Loop

```rust
// regime_deep_loop
// 1. Her tick'te tüm enabled semboller × configured timeframes için regime hesapla
// 2. regime_snapshots tablosuna yaz
// 3. Önceki snapshot ile karşılaştır → transition tespiti
// 4. Transition alert → notify_outbox (Telegram)
// 5. Multi-TF confluence hesapla → dominant regime belirle
// 6. Adaptive parametre lookup → sonraki detector/strategy pass'ında kullanılır
// 7. HMM (enabled ise): retrain + predict → hmm_state + hmm_confidence yaz
```

---

## 9. Test Planı

- **Unit**: Regime hesaplama her gösterge kombinasyonu için
- **Multi-TF**: Farklı TF'lerde farklı rejimler → doğru confluence skoru
- **Transition**: Squeeze→Trending geçiş tespiti (tarihsel veri ile)
- **HMM**: Train/predict cycle, transition probability doğrulama
- **Adaptive**: Rejim değişince parametre override doğru uygulanıyor mu
- **GUI E2E**: Heatmap, timeline, performance tablo render

---

## 10. Tahmini Efor

| Adım | Efor |
|------|------|
| Multi-TF regime hesaplama + confluence | 1 gün |
| Regime transition detection | 1 gün |
| regime_snapshots DB + worker loop | 0.5 gün |
| Transition DB + alerting (Telegram) | 0.5 gün |
| Adaptif parametre sistemi + override tablo | 1 gün |
| Strategy regime-aware selection | 0.5 gün |
| HMM implementasyonu | 2 gün |
| API endpoint'leri (8 adet) | 1 gün |
| GUI: Dashboard + Heatmap | 1.5 gün |
| GUI: Timeline overlay | 0.5 gün |
| GUI: Performance correlation | 0.5 gün |
| GUI: Strategy recommendation panel | 0.5 gün |
| Config seed + migration'lar | 0.5 gün |
| Testler | 1 gün |
| **TOPLAM** | **~12 gün** |
