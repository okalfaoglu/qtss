# Faz 10 — Pattern Completion Plan

**Amaç:** Kullanıcının referans listesindeki TÜM formasyonların (Reversal, Continuation, Triangle, Harmonic, Candlestick, Gap, Elliott) sisteme eklenmesi **ve** web chart üzerinde doğru geometriyle çizim olarak gösterilmesi. Bu belge hem detector eklemeyi hem de GUI overlay katmanını kapsar — iki iş paralel aşamalarda ilerler. Her detector CLAUDE.md kurallarına (#1 dispatch-table, #2 DB-config, #3 katman ayrımı) uygun eklenir ve kabul kriterlerinden geçmeden registry'ye bağlanmaz.

**Giriş durumu (audit sonrası):**
- Classical: 21/29 (8 eksik)
- Harmonic: 10/10 ✅
- Elliott: 6/7 (running_triangle eksik)
- Gap: 0/5 ❌
- Candlestick: 0/~25 ❌

---

## 1. Aşama — Classical Boşluklar (8 detector)

Crate: `qtss-classical`. Dosya: `src/shapes.rs` (registry) + config key'leri `migrations/0140_classical_faz10_config.sql`. Feature source etiketi: `crates/qtss-worker/src/feature_sources/classical.rs` SHAPE_TABLE + `pretty_subkind` satırları.

| Subkind | Geometri özeti | Yeni config anahtarları |
|---|---|---|
| `triple_top` | 3 ardışık HH pivot, |H1−H2|,|H2−H3| < `triple.peak_tol_atr` × ATR; araya 2 LL boyun çizgisi | `classical.triple.peak_tol_atr`, `classical.triple.min_span_bars`, `classical.triple.neckline_slope_max` |
| `triple_bottom` | Yukarıdakinin aynadaki hali (LL pivot) | paylaşır |
| `broadening_top` | 2 HH pivot (H2>H1) + 2 LL pivot (L2<L1); iki çizgi birbirinden **uzaklaşıyor**; downtrend kontex yok | `classical.broadening.min_pivots`, `classical.broadening.slope_min_atr_per_bar`, `classical.broadening.max_overlap_bars` |
| `broadening_bottom` | Ayna | paylaşır |
| `broadening_triangle` | Broadening yapı fakat bir kenar yatay → diverging triangle; apex geride | `classical.broadening_triangle.horizontal_slope_max_atr` |
| `v_top` | Tek-bar ya da 2-bar sharp reversal: prior N-bar up-run ≥ `v.prior_move_atr`, ardından ≥ `v.reversal_atr` ters hareket, reversal bar range ≥ `v.spike_range_atr` | `classical.v.prior_move_atr`, `classical.v.reversal_atr`, `classical.v.spike_range_atr`, `classical.v.lookback_bars` |
| `v_bottom` | Ayna | paylaşır |
| `measured_move_abcd` | A→B impulse, B→C retrace ∈ [`abcd.c_min_retrace`, `abcd.c_max_retrace`] × AB, C→D = AB × `abcd.d_projection_tol` (default 1.0 ±10 %) | `classical.abcd.c_min_retrace`, `classical.abcd.c_max_retrace`, `classical.abcd.d_projection_tol`, `classical.abcd.min_bars_per_leg` |

### Doğrulama Adımları (her detector için çalıştırılır, kabul kriteri = hepsi geçmeli)

1. **Unit test — pozitif synthetic:** Elle üretilmiş "kesinlikle bu desen" pivot dizisinde detector `Some` döner ve `structural_score > 0.5`.
2. **Unit test — negatif synthetic:** Bir eşiği kasıtlı ihlal eden dizide (ör. triple top'ta 3. tepe `peak_tol_atr`'yi aşıyor) `None` döner.
3. **Config binding testi:** Defaults migration'dan yüklenen config ile detector run edilir; hard-code yok kontrolü (`grep -n` ile pivot eşiklerinin tamamı config key üzerinden geliyor).
4. **Feature snapshot testi:** `feature_sources/classical.rs` SHAPE_TABLE ordinal'ı unique ve `pretty_subkind` Türkçe/İngilizce label üretiyor.
5. **Replay smoke testi:** 1 haftalık BTC/ETH 1h/4h bar verisi üzerinde detector çalıştırılıp `qtss_v2_detections` içine kaç satır düştüğü ve `structural_score` dağılımı (p50/p90) loglanır. p50 < 0.3 veya > 0.9 ise eşikler config-editable olarak raporlanır (manuel tuning önerisi).
6. **Çakışma testi:** Aynı zaman penceresinde triple_top ve double_top birlikte tetiklenmemeli (triple_top önce kontrol edilip eğer eşleşirse double_top skor penaltysi). Yoksa AI eğitiminde aşırı-korelasyon olur.
7. **Geometri invariant testi:** `broadening`'te üst çizgi eğimi > 0, alt çizgi eğimi < 0 assertion'ı; `v_top`'ta reversal bar close < prior bar close assertion'ı.

Çıktı: Kabul kriterleri geçtikten sonra `FEATURE_SOURCES` etkiler, migration run'lanır, worker redeploy edilir. 24 saat prod log monitörü: `journalctl -u qtss-worker | grep -i "subkind=(triple|broadening|v_top|v_bottom|abcd)"` — zero panic, insert error < 1 %.

---

## 2. Aşama — Gap Motoru (yeni crate `qtss-gap`, 5 detector)

Bağımlılıklar: `qtss-bar` (OHLCV), `qtss-indicators` (ATR, volume SMA), `qtss-confluence`.

| Subkind | Tanım | Temel koşul |
|---|---|---|
| `common_gap` | Sıradan boşluk, range-bound içinde | `|open[t] − close[t-1]| > config.min_gap_atr`, trend context=range |
| `breakaway_gap` | Desen/trend kırılımında boşluk | Aynı koşul + prior N-bar range kırılımı + `volume[t] > config.vol_mult × volsma` |
| `runaway_gap` | Güçlü trend ortasında boşluk | Trend context=trend, önceki breakaway'den sonra, ATR/volume sağlaması |
| `exhaustion_gap` | Trend sonunda boşluk | Trend context=trend, RSI extreme + volume spike + sonraki 3 bar reversal riski |
| `island_reversal` | Breakaway gap + N-bar island + ters breakaway gap | State machine: open_gap → island_bars → close_gap (ters yön) |

### Doğrulama Adımları

1. Synthetic bar dizileri ile 5 pozitif + 5 negatif birim test (unit).
2. `island_reversal` state machine: her state geçişi log'a yazılır, test'te state sequence assert.
3. Volume confirmation config'le kapatılabilir; kapalıyken daha çok sinyal — config-table seed'de `vol_mult=1.5` default.
4. Gap sınıflandırma dispatch-table (CLAUDE.md #1): `fn classify_gap(ctx) -> GapKind` içinde `match` değil `dispatcher.evaluate()` loop.
5. Feature source `qtss-worker/src/feature_sources/gap.rs` — gap_size_atr, volume_ratio, post_gap_followthrough features.
6. FK join testi: `qtss_v2_detections.family='gap'` kayıtları için `qtss_features_snapshot` satırı oluşuyor.
7. Replay smoke: 30 günlük 1h BTC barlarında her gap tipinden ≥ 1 tetiklemesi (veri sparsity kontrolü).

---

## 3. Aşama — Candlestick Motoru (yeni crate `qtss-candles`, ~25 detector)

Mimari: `CandleDetector` trait + tek-bar / iki-bar / üç-bar impl'leri. Dispatch tablosu `CANDLE_REGISTRY: &[&dyn CandleDetector]`. Config: gövde/fitil oran eşikleri per-tip.

### Detector Listesi

**Tekli (7):**
- `doji_standard`, `doji_long_leg`, `doji_gravestone`, `doji_dragonfly`
- `hammer`, `inverted_hammer`, `hanging_man`, `shooting_star`
- `marubozu_bull`, `marubozu_bear`, `spinning_top`

**İkili (8):**
- `engulfing_bull`, `engulfing_bear`
- `harami_bull`, `harami_bear`
- `tweezer_top`, `tweezer_bottom`
- `piercing_line`, `dark_cloud_cover`
- `on_neck`, `in_neck`, `thrusting_line`

**Üçlü (8):**
- `morning_star`, `evening_star`, `abandoned_baby_bull`, `abandoned_baby_bear`
- `three_white_soldiers`, `three_black_crows`
- `three_inside_up`, `three_inside_down`, `three_outside_up`, `three_outside_down`
- `tri_star_bull`, `tri_star_bear`

**Bonus (multi-bar):**
- `rising_three_methods`, `falling_three_methods`

### Doğrulama Adımları

1. Her detector için **TradingView / Bulkowski** referans formül karşılaştırması — yorumda atıf.
2. Body/wick oranları `qtss_config` içinde `candles.body_min_pct`, `candles.wick_max_pct` gibi anahtarlarla — hard-code yok.
3. Trend-context filter'ı: Hammer yalnız downtrend'de, hanging_man yalnız uptrend'de sinyal kabul edilir. Context = prior-N-bar SMA slope.
4. Overlap kuralı: Aynı bar bitiminde tetiklenen detector'lar dispatch sırası ile çelişmemeli (ör. "doji" + "marubozu" aynı anda çıkamaz). Unit test çakışma matrisini kontrol eder.
5. Synthetic bar fixture'ları `crates/qtss-candles/tests/fixtures/*.json` — pozitif + negatif örnekler.
6. Feature source: `feature_sources/candles.rs` — candle_kind_ordinal, body_pct, upper_wick_pct, lower_wick_pct, context_ordinal.
7. Replay smoke: 1 haftalık 15m BTC → her kategori (tekli/ikili/üçlü) için en az 5 tetikleme olmalı, yoksa eşikler config-tune edilir.

---

## 4. Aşama — Küçük Ekler

- `qtss-elliott` → `running_triangle` subkind (corrective triangle'ın E-dalgasının B-yi aştığı alt-tip): mevcut triangle detector'a flag param, config `elliott.triangle.running_allow`.
- `qtss-classical` → `horizontal_channel` rectangle alias'ı (direction=0, slope=0) — aynı detector'a subkind çıkışı eklenir.
- `qtss-classical` → `pennant_bearish` mevcut `pennant` detector'ının direction=-1 koluna ayrı subkind etiketi.
- `qtss-classical` → `scallop_rising`, `scallop_falling` — U-şekilli düşük amplitüdlü devam yapısı (rounding bottom + breakout hybrid).

---

## Genel Kabul Kriterleri

Her aşama kapanış şartları:

| # | Kriter | Komut |
|---|---|---|
| 1 | `cargo build --release -p <crate>` zero warning | build |
| 2 | `cargo test --release -p <crate>` tüm test yeşil | test |
| 3 | Migration idempotent (iki kez run edilebilir) | `psql -c "\\d qtss_config"` + rerun |
| 4 | CLAUDE.md #2 uyumu: `rg -n 'const .*: f64 = [0-9]' crates/<crate>/src/` sadece `PI` / indicator periyot şeklinde olmalı; iş eşiği hardcode yok | grep |
| 5 | Feature source kayıt: `FEATURE_SOURCES` array'i + `SHAPE_TABLE`/equivalent ordinal unique | grep |
| 6 | `pretty_subkind` tüm yeni subkind'lar için label üretiyor | unit test |
| 7 | 24 saat prod log takibi: panic yok, insert error < 1 % | journalctl |
| 8 | Baseline LightGBM eğitiminde yeni feature column'lar görünüyor (`qtss-trainer stats`) | stats |

## Rollback Planı

Bir detector prod'da yanlış davranırsa (false positive fırtınası):
1. Config GUI'den `feature_store.sources.<src>.enabled=false` yap — FS yazımı durur.
2. Detector'ı orchestrator DetectorRunner listesinden çıkar (config key `detectors.<name>.enabled=false`).
3. Yeni eşik değerlerle tekrar aç — migration değişikliği gerekmeden canlıda tune edilir.

---

## 5. Aşama — Web GUI Pattern Çizim Katmanı (Paralel Stream)

Her detector yeni bir formasyon yayınladığında web chart'ında doğru geometri overlay'i çizilmeli. Veri ≠ görsel ayrımı korunur (CLAUDE.md #3): backend renderable-geometry alanı üretir, frontend sadece çizer.

### 5.1 Data Contract

`qtss_v2_detections.anchors` zaten `[{bar_index, price, ts_ms, label}]` formatında tutuluyor. Rendering için yetersiz olan 3 ek alan:

| Alan | Kaynak | Örnek |
|---|---|---|
| `render_geometry` JSONB | Detector → Detection insert | `{"kind":"polyline","points":[...]}` / `{"kind":"two_lines","upper":[a,b],"lower":[c,d]}` / `{"kind":"horizontal_band","y_upper":..,"y_lower":..,"x_from":..,"x_to":..}` / `{"kind":"arc","center":[x,y],"radius":..,"start_angle":..,"end_angle":..}` |
| `render_style` TEXT | Detector family | `reversal_bearish`, `continuation_bull`, `range_neutral`, `harmonic_xabcd` |
| `render_labels` JSONB | Detector | `{"neckline":72300,"tp":74150,"invalidation":71900}` |

**Migration:** `0141_detections_render_fields.sql` — yeni kolonları NULL default olarak ekler, mevcut satırları bozmaz.

### 5.2 Render Kind Katalogu (Dispatch-table — CLAUDE.md #1)

| `kind` | Kullanan formasyonlar | Çizim |
|---|---|---|
| `polyline` | Elliott waves, XABCD harmonics, ABCD measured move | Sıralı noktalar arası düz çizgi + etiket |
| `two_lines` | Triangles, channels, wedges, broadening, flag, pennant | Üst ve alt trend çizgileri; breakout tarafı highlighted |
| `horizontal_band` | Rectangle, horizontal_channel, range zones (FVG, OB, liquidity) | Yatay bant + opacity |
| `head_shoulders` | H&S, Inverse H&S, Triple Top/Bottom | 5/6 anchor + neckline |
| `double_pattern` | Double Top/Bottom | 3 anchor + neckline |
| `arc` | Rounding, Cup & Handle, Scallop | Kavis (quadratic bezier) + handle segment |
| `v_spike` | V-Top, V-Bottom, Spike | Tek pivot + iki yön oku |
| `gap_marker` | Gaps | Gap range'i highlight + kategoriye göre renk |
| `candle_annotation` | Candlestick patterns | Tek/iki/üç bar'lık vurgu + tip-etiketi |
| `diamond` | Diamond top/bottom | 4 anchor'dan eşkenar dörtgen |
| `fibonacci_ruler` | Harmonic ratios, ABCD retrace | Fibonacci seviye çizgileri |

### 5.3 Backend

- `qtss-classical::render` modülü eklenir; her detector `ShapeMatch::render()` implemente eder veya spec satırına `render_fn: fn(&ShapeMatch, &[Pivot]) -> RenderGeometry` eklenir.
- Orchestrator detection insert'inde `render_geometry` doldurulur (none ise `NULL`).
- API `GET /v2/detections?symbol=...&from=...&to=...` yanıtına `render_geometry` serialize edilerek döner.

### 5.4 Frontend (React + TanStack)

- Yeni hook: `usePatternOverlays(symbol, tf, range)` — detections endpoint'ini tüketir.
- Yeni component: `<PatternLayer/>` — lightweight-charts v4 primitive layer (custom series via `ICustomSeriesPaneView`) ile overlay'leri çizer.
- Render dispatch: `RENDER_KIND_REGISTRY: Record<string, (ctx, geo, style) => void>` — her kind için küçük bir canvas-draw fn. Yeni kind eklemek = tek satır, central `switch` yok.
- Style paleti `tailwind.config` üzerinden tema-duyarlı: bullish=emerald-500, bearish=rose-500, neutral=slate-400, harmonic=violet-500, gap=amber-500, candle=cyan-500.
- Kullanıcı kontrol paneli: "Reversal / Continuation / Triangle / Harmonic / Gap / Candle" toggle'ları — localStorage'da kalıcı.
- Click handler: overlay'e tıklayınca sağda detay paneli (pattern adı, score, anchor listesi, trade levels).

### 5.5 Kabul Kriterleri (Çizim)

1. Her yeni detector için en az 1 Storybook benzeri fixture: mock detection + beklenen SVG snapshot (Vitest + `@testing-library/jest-dom`).
2. Replay: 1 haftalık BTC 1h veri + overlay görsel inceleme → false positive geometri yok (ör. upside-down neckline).
3. Performance: 500 concurrent overlay ≤ 16 ms frame time (60 fps).
4. A11y: her overlay keyboard-focusable, `aria-label="Pattern: Triple Top, bearish, score 0.72"`.
5. Mobile viewport < 640 px: overlay'ler otomatik sadeleşir (sadece ana çizgi + etiket).

### 5.6 Eforlama

- 5.a — Backend render_geometry alanı + migration: **2 saat**
- 5.b — Her detector için render_fn yazımı: **~30 dakika × 50 detector = 25 saat** (aşamalar boyunca paralel ilerler)
- 5.c — Frontend `<PatternLayer/>` + dispatcher + ilk 4 kind: **4 saat**
- 5.d — Geriye kalan render kind'lar + testler: **6 saat**

**Toplam:** ~37 saat, aşama başına paralel küçük PR'lar halinde ilerler.

---

## Zaman Kestirimi

| Aşama | Efor | Sat. kestirim |
|---|---|---|
| 1 — Classical 8 detector | ~600 satır + test | 1 oturum |
| 2 — Gap crate + 5 detector | ~900 satır + yeni crate iskeleti | 1 oturum |
| 3 — Candlestick crate + ~25 detector | ~2000 satır + test fixture | 2 oturum |
| 4 — Küçük ekler | ~200 satır | 1 oturum içinde artan iş |
| 5 — GUI çizim katmanı (paralel) | ~1500 satır (backend render_fn + frontend PatternLayer) | Her aşamayla birlikte +1 oturum |

**Toplam:** ~5-6 odaklı oturum. Bu belge her oturum başında checklist olarak kullanılır; tamamlanan madde `[x]` işaretlenir.

---
_Last updated: 2026-04-18 — Faz 10 kickoff._
