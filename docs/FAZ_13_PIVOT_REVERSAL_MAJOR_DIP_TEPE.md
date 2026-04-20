# Faz 13 — Pivot-Tabanlı Major Dip/Tepe Tespit Sistemi

> **Statü:** Draft / Locked spec
> **Sahip:** QTSS Core
> **Bağımlılıklar:** Faz 12.E (çok-seviyeli backtest boru hattı), `qtss-pivots` (ATR ZigZag L0–L3 + `classify_swing`), `qtss_v2_detections`, Rapor sayfası.
> **Bu Faz'ın amacı:** L0/L1 "tepki" dip-tepelerini L2/L3 "major" dönüşlerden ayıran, market-yapısı (BOS/CHoCH) + swing klasifikasyonu (HH/HL/LH/LL) + prominence tabanlı, asset-class agnostik, config-driven bir dönüş-tespit ailesi inşa etmek; bunu sweep → outcome-eval → Rapor → Chart bütün zinciri boyunca birinci sınıf vatandaş yapmak.

---

## 1. Motivasyon

TBM indikatör-tabanlı ve **tepki** (reaktif) bölgeleri yakalar; gerçek trend dönüşlerini (CHoCH) kaçırır. Operatör iki farklı karar verir:

| Soru | Doğru katman |
|------|--------------|
| "Şimdi kısa vadeli scalp için dipten long?" | **Tepki** — L0/L1 |
| "Trend döndü mü, pozisyonu büyütmeli miyim?" | **Major** — L2/L3 |

Pivot ağacı (L0 fib×2, L1 fib×3, L2 fib×5, L3 fib×8) zaten bu ayrımı veriyor: büyük çarpanlı seviyeler daha anlamlı, az sayıda, daha büyük fiyat hareketlerini kapsıyor. Faz 13 bu bilgiyi **tier** olarak sisteme açık sinyal haline getiriyor.

## 2. Kavram Sözlüğü

- **Tepki (reactive) dip/tepe:** L0, L1 seviye pivotları. Yüksek frekans, küçük hareket. Ortalama ömür birkaç bar – birkaç gün.
- **Major (structural) dip/tepe:** L2, L3 seviye pivotları. Düşük frekans, büyük hareket. Gerçek dönüş beklentisi taşır.
- **BOS (Break of Structure):** Trend devam sinyali. HH→HH, HL→HL (boğa); LL→LL, LH→LH (ayı).
- **CHoCH (Change of Character):** Trend dönüşü. LH zinciri sonrası HH ⇒ *boğa CHoCH, önceki Low major dip*. HL zinciri sonrası LL ⇒ *ayı CHoCH, önceki High major tepe*.
- **Tier score:** CHoCH > BOS > Neutral. Rapor'da filtre ve sıralama için.

## 3. Detektör Tasarımı (`pivot_reversal` family)

### 3.1 Sorumluluk (CLAUDE.md #3)

Detektör **sadece "majör/reaktif dip-tepe gördüm" der**. Validator/target/risk bilmez. Veri kaynağı **yalnızca** `pivot_cache` tablosu — borsa/asset class bilmez (CLAUDE.md #4).

### 3.2 Subkind taksonomisi

```
{tier}_{event}_{direction}_{level}

tier       := reactive | major
event      := choch | bos | neutral
direction  := bull | bear
level      := L0 | L1 | L2 | L3
```

Örnekler:

- `major_choch_bull_L3` — L3 seviyesinde, büyük trend dönüşü (boğa). En yüksek öncelik.
- `reactive_bos_bear_L0` — L0 seviyesinde, ayı devamı. Sadece kısa süreli scalp.
- `reactive_choch_bull_L1` — tepki zirvesinde mikro dönüş. Orta öncelik.

### 3.3 Tier eşlemesi

| Level | Tier     | CHoCH score | BOS score | Neutral score |
|-------|----------|-------------|-----------|---------------|
| L0    | reactive | 0.50        | 0.30      | 0.15          |
| L1    | reactive | 0.65        | 0.40      | 0.20          |
| L2    | major    | 0.85        | 0.60      | 0.30          |
| L3    | major    | 0.95        | 0.70      | 0.40          |

Skorlar **`qtss_config`** içinde (CLAUDE.md #2). Kod seviyesinde sabit **yok**.

### 3.4 Anchors

Üç çapa yazılır (R-multiple outcome-eval buna uyum sağlar):

1. `prev_opp` — invalidation (prior opposite pivot)
2. `entry_anchor` — gerçek major pivot (CHoCH için `prev_opp`, aksi halde `curr`)
3. `confirm` — CHoCH/BOS'u doğrulayan bar (walk-forward geçerli `detected_at`)

### 3.5 Klasifikasyon

Polimorfizm / look-up tablosu (CLAUDE.md #1 — `if/else` minimizasyonu):

```rust
static STRUCTURE_MAP: once_cell::sync::Lazy<HashMap<(&str,&str), StructureEvent>> = ...
// ("HH","LH") -> ChochBull
// ("LL","HL") -> ChochBear
// ("HH","HH") | ("HL","HL") -> BosBull
// ("LL","LL") | ("LH","LH") -> BosBear
```

## 4. Sweep Binary (`pivot-reversal-backtest-sweep`)

### 4.1 Davranış

- Input: `pivot_cache` (exchange, symbol, timeframe, level).
- Idempotent: slice başına DELETE + bulk INSERT.
- Filtreler: `PIVOT_REVERSAL_SYMBOLS`, `PIVOT_REVERSAL_INTERVALS`, `PIVOT_REVERSAL_MIN_PROMINENCE_{L0..L3}`.
- Output: `qtss_v2_detections(mode='backtest', family='pivot_reversal', subkind=<yeni taksonomi>, pivot_level, anchors, invalidation_price, structural_score, raw_meta)`.

### 4.2 `raw_meta` şeması

```json
{
  "run_id": "...",
  "sweep": "pivot_reversal_backtest_sweep",
  "tier": "major",
  "bearish": false,
  "event": "choch_bull",
  "tier_score": 0.90,
  "prominence": 3.7,
  "swing_type_curr": "HH",
  "swing_type_prev_opp": "LL",
  "swing_type_prev_same": "LH"
}
```

### 4.3 Sıralı iş akışı

1. `list_series` — pivot_cache distinct (exchange, symbol, tf)
2. Her seviye için prominence floor uygula
3. Sliding window (prev_opp, prev_same, curr) üret
4. `classify()` ile StructureEvent
5. Tier × level × event çarpımıyla score
6. `insert_detection` — 3 anchor + meta

## 5. Outcome Evaluator Entegrasyonu

`backtest-outcome-eval` zaten family-agnostik. Değişiklik:

- `EVAL_FAMILIES` default listesine `pivot_reversal` eklendi.
- **Tier-bazlı TP/SL override** (yeni):
  - `reactive` → TP1=1.0R, TP2=2.0R, expiry_bars_mult=20
  - `major`    → TP1=1.5R, TP2=3.0R, expiry_bars_mult=60
  - Değerler `qtss_config` (`eval.pivot_reversal.reactive.tp1_r` vb.).

## 6. Rapor Sayfası

`/v2/reports/backtest` üzerinde:

1. **Üst bant — Tier dashboard.** İki card: *Reactive (L0+L1)* vs *Major (L2+L3)* — adet, WR, avg pnl, expected R.
2. **Family grid.** Mevcut. `pivot_reversal` ayrı card.
3. **Level breakdown.** Her level için CHoCH vs BOS vs Neutral ayrımı (subkind prefix'ten). TP1/TP2 hit, WR, median pnl.
4. **Subkind tablosu.** Sortable: adet, WR, TP1%, TP2%, median pnl. Tier ve event rozetli.
5. **Filtre barı.** Tier (reactive/major/all), Event (choch/bos/neutral), Level (L0..L3), Direction (bull/bear).

### 6.1 API

`GET /v2/reports/backtest-performance?tier=major&event=choch` — yeni query param'lar. Endpoint `v2_reports.rs` içinde tier bucket'ı subkind prefix'ten regex ile türetecek; ek kolon yok (backward compatible).

## 7. Chart Entegrasyonu

- `FAMILY_COLORS.pivot_reversal` mevcut (cyan).
- **Tier görselleştirme:** major = full opacity + kalın marker, reactive = %60 opacity + ince marker.
- **Event işareti:**
  - CHoCH → yıldız (★) marker
  - BOS → üçgen
  - Neutral → nokta
- State bucket toggle (active/completed/cancelled) mevcut, korunur.
- InfoPanel: tier + event + swing_type zinciri + R-multiple hedefler + outcome rozeti.

## 8. Config (CLAUDE.md #2)

`qtss_config` içinde yeni namespace `pivot_reversal.*`:

| Key | Default | Açıklama |
|-----|---------|----------|
| `pivot_reversal.tier.reactive.levels` | `["L0","L1"]` | Reactive sayılan seviyeler |
| `pivot_reversal.tier.major.levels` | `["L2","L3"]` | Major sayılan seviyeler |
| `pivot_reversal.score.L{0..3}.choch` | tablo §3.3 | CHoCH tier skoru |
| `pivot_reversal.score.L{0..3}.bos` | tablo §3.3 | BOS tier skoru |
| `pivot_reversal.score.L{0..3}.neutral` | tablo §3.3 | Neutral tier skoru |
| `pivot_reversal.prominence_floor.L{0..3}` | 0.0 | Seviye başı minimum prominence |
| `eval.pivot_reversal.reactive.tp1_r` | 1.0 | Tepki TP1 (R) |
| `eval.pivot_reversal.reactive.tp2_r` | 2.0 | Tepki TP2 (R) |
| `eval.pivot_reversal.major.tp1_r` | 1.5 | Major TP1 (R) |
| `eval.pivot_reversal.major.tp2_r` | 3.0 | Major TP2 (R) |
| `eval.pivot_reversal.reactive.expiry_bars_mult` | 20 | |
| `eval.pivot_reversal.major.expiry_bars_mult` | 60 | |

Tümü Web GUI Config Editor'dan canlı güncellenebilir; audit log'a yazılır.

## 9. Migration planı

- **0194_pivot_reversal_config_seed.sql** — yukarıdaki default key'leri `qtss_config`'a seed et.
- **0195_qtss_v2_detections_subkind_index.sql** — `(family, pivot_level, subkind)` üzerine partial index (Rapor sorguları için).
- **0196_backtest_outcomes_tier_view.sql** — `v_pivot_reversal_tier_performance` view: subkind prefix'ten tier türeten materialized view.

## 10. Test Planı

### 10.1 Birim testler (qtss-worker crate)

- `classify((HH, LH)) == ChochBull`
- `classify((LL, HL)) == ChochBear`
- `classify((HH, HH)) == BosBull`
- `classify((LL, LH)) == BosBear`
- Tier look-up: `tier_for(L0) == Reactive`, `tier_for(L3) == Major`.

### 10.2 Entegrasyon

- Sabit bir mini dataset (BTCUSDT 1h, 1000 bar) üzerinde sweep → tam olarak N `major_choch_*_L3` + M `reactive_*_L0` bekleniyor. Snapshot'la karşılaştır.
- Outcome-eval sonrası tier'a göre WR ve R-multiple beklenti aralıkları.

### 10.3 UI

- Reports'ta tier filter → URL query param → endpoint'e gitmiyor olursa fail.
- Chart'ta major CHoCH marker kalın, reactive BOS marker ince.

## 11. Ölçüm / Başarı Kriterleri

| Metrik | Hedef |
|--------|-------|
| Major CHoCH WR (13 sembol × tüm TF) | ≥ %55 |
| Major CHoCH expected R | ≥ +0.35 |
| Reactive CHoCH WR | ≥ %48 |
| Reactive BOS WR | ≥ %42 (trend takibi) |
| Rapor sayfası load < | 800 ms (indexli) |
| Config değişikliği → outcome-eval rerun | < 5 dk full sweep BTC 1h |

## 12. Uygulama Sırası (Checklist)

- [ ] Migration 0194/0195/0196 yaz ve apply
- [ ] `qtss-config`: `pivot_reversal.*` tipli wrapper (`PivotReversalConfig::load(pool)`)
- [ ] `pivot_reversal_backtest_sweep.rs`:
  - [ ] `Tier` enum + look-up map
  - [ ] `subkind` formatını `{tier}_{event}_{direction}_{level}`'e güncelle
  - [ ] Skor tablosunu config'ten oku (hardcode kaldır)
  - [ ] `raw_meta.tier`, `raw_meta.event`, `raw_meta.direction` ekle
- [ ] `backtest_outcome_eval.rs`:
  - [ ] Tier'a göre TP/SL override
  - [ ] Expiry_bars_mult override
- [ ] `v2_reports.rs`:
  - [ ] tier/event/direction query param'ları
  - [ ] Tier bucket aggregate
- [ ] `Reports.tsx`:
  - [ ] Tier dashboard card'ları
  - [ ] Filter bar
  - [ ] Subkind tablo rozetleri
- [ ] `Chart.tsx`:
  - [ ] Tier opacity / marker kalınlığı
  - [ ] Event shape (star / triangle / dot)
  - [ ] InfoPanel tier + event + swing zinciri
- [ ] Testler (10.1–10.3)
- [ ] 13 sembol × tüm TF full sweep
- [ ] Outcome-eval full run
- [ ] Ölçüm toplama (§11) — pass/fail raporu

## 13. Risk & Karşı-argümanlar

- **Pivot gürültüsü L0'da çok yüksek** → prominence floor + tier-bazlı skor onu Rapor'da doğal olarak arka plana iter. Operatör L0 WR'yi gördüğünde scalp dışında kullanmayacak.
- **CHoCH "yalancı" olabilir** → invalidation fiyatı `prev_opp` olarak yazılı; R-multiple doğrudan cezalandırıyor. Rapor'daki expected-R değeri bunu çıplak gösterir.
- **Sembole göre tier anlamı değişir** (BTC L3 ≠ memecoin L3) → ilerde `pivot_reversal.tier.<symbol_class>.levels` override. Şimdilik default yeterli.
- **Config değişikliği skorları bozabilir** → audit log + "dry preview" (config editor'da değişiklik öncesi mini-eval). İleri aşama.

## 14. Sonraki Fazlarla Kesişim

- **Faz 8 Setup Engine:** `major_choch_*` → Q-RADAR "Quality" profili için direct feed. `reactive_*` → "Tactical" profile.
- **Faz 11 Regime Deep:** Regime filter (`trending` rejimde BOS ağırlığı ↑, `ranging` rejimde CHoCH ağırlığı ↓) — `pivot_reversal.score`'u rejim-koşullu çarpanla modifiye et.
- **Faz 9 X Abonelik:** `major_choch_bull_L3` confirmation → otomatik X post şablonu (abonelere özel).

## 15. Geçersiz Sayılacak Önceki Davranış

- Önceki `pivot_reversal` subkind formatı `{event}_{direction}_{level}` (tier yok) → **deprecated.** Migration sonrası sweep yeniden çalıştığında yeni taksonomi overwrite eder (idempotent DELETE+INSERT). Rapor endpoint'i eski formatı tanımaya devam etmez; geçiş anında sweep çalıştırılmadıysa rapor o aile için boş görünür (kasıtlı — yanlış etiketli veri üretmemek için).
