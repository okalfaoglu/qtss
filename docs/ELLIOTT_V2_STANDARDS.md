# Elliott V2 Standards

This document defines naming, coloring, wave labels, hard rules, and ZigZag scope for the Elliott V2 engine.

## 1) Timeframe Hierarchy

- Macro: `4h`
- Intermediate: `1h`
- Micro: `15m`

The chart anchor is `15m`. `1h` and `4h` are rendered as overlays on the same chart.

## 2) Naming and Labeling

- `4h` labels (default glyphs in `adapter.ts`):
  - Impulse: `① ② ③ ④ ⑤`
  - Corrective: `Ⓐ Ⓑ Ⓒ`
- `1h` labels:
  - Impulse: `(1) (2) (3) (4) (5)`
  - Corrective: `(A) (B) (C)`
- `15m` labels:
  - Impulse: `i ii iii iv v`
  - Corrective: `a b c`

Nested formations (optional, `show_nested_formations`):

- Nested impulse legs (wave 1/3/5 internals): `n1..n5`, `u1..u5`, `v1..v5`
- Nested corrective inside wave 2/4: `w2·a w2·b w2·c`, `w4·a w4·b w4·c` (marker text uses the same glyph mapping per TF)

## 3) Color Standard

Default colors (see `web/src/lib/elliottWaveAppConfig.ts` → `DEFAULT_ELLIOTT_MTF_WAVE_COLORS`):

- Macro `4h`: `#e53935` (red)
- Intermediate `1h`: `#43a047` (green)
- Micro `15m`: `#fb8c00` (orange)

Notes:

- Projection layers (`elliott_projection*`) typically reuse the TF base color but with dashed/dotted styles; active-C highlight uses the chart layer palette (`TvChartPane.tsx`).
- Colors are configurable via `ElliottWaveCard` settings (MTF wave/label/zigzag colors). Keep this section in sync with config defaults when changing palette.

## 4) Elliott Rule Scope (V2.0)

### Hard invalidation rules for impulse

1. Wave 2 cannot retrace beyond the start of Wave 1.
2. Wave 3 cannot be the shortest among Waves 1, 3, 5.
3. **Standard** impulse: wave 4 cannot overlap wave 1 price territory (`impulse.ts` — `w4_no_overlap_w1`). **Diagonal** impulse is a separate variant: overlap allowed; extra rules (`ed_r4_w3_area_gt_w2`, `w5_not_longest_135`, `ld_r3_w5_ge_1382_w4`, `w4_diagonal_mode`). It is **implemented** but **off by default** in the pattern menu (`motive_diagonal: false` in `elliottPatternMenuCatalog.ts`).

### Corrective scope

- Enabled: `ABC`, `zigzag (5-3-5)`, `flat (3-3-5)`, `triangle (3-3-3-3-3)`, `W-X-Y / W-X-Y-X-Z`.
- Notes:
  - Triangle detection is constrained to wave-4 / post-impulse corrective context (not standalone wave-2 default).
  - Complex combinations are scored as candidates/confirmed via ratio + connector checks.
  - Diagonal motive is optional (menu); standard motive stays the default.

## 5) ZigZag Policy

- Elliott decision engine uses one ZigZag logic per timeframe.
- Channel module can keep multiple ZigZag configs for drawing/scanning.
- Channel ZigZag output must not directly invalidate Elliott state.

## 6) State Model

- Parent-child chain: `4h -> 1h -> 15m`.
- If micro count invalidates:
  - reset only micro state,
  - keep parent states unless parent hard rules fail.

## 7) Confidence and Decisions

- Decision class in V2.0: `invalid`, `candidate`, `confirmed`.
- Confidence should be derived from:
  - hard rules pass/fail,
  - structure completeness,
  - ratio proximity (soft scoring).

## 8) Implementation map (web — drift control)

This document is the **normative** label/color/rule set. The running engine lives under `web/src/lib/elliottEngineV2/`:

| Topic | Primary code |
|-------|----------------|
| Timeframe type + MTF bucketing (`15m` anchor → `15m`+`1h`+`4h`, …) | `types.ts`, `mtf.ts` |
| Impulse / corrective wave logic | `impulse.ts`, `corrective.ts` |
| Hard checks (compression, overlap-style guards) | `tezWaveChecks.ts`, `engine.ts` |
| ZigZag-driven pivots | `zigzag.ts` |
| Chart overlays: labels, line styles, per-TF coloring hooks | `adapter.ts` (label glyphs per TF; colors often supplied via `elliottWaveAppConfig` / chart layer) |
| Engine orchestration, state merge across TFs | `engine.ts` |
| **Forward projection (İleri Fib / Elliott projeksiyon)** | `projection.ts` — `buildElliottProjectionOverlayV2` — behavior **§8.1**, limits / roadmap **§8.2** |
| Public exports | `index.ts` |

### 8.1 Forward projection (`projection.ts`) — normative behavior

Chart overlay `zigzagKind: elliott_projection` (isteğe bağlı ikinci yol: `elliott_projection_alt`) ve varsa `elliott_projection_done` / `elliott_projection_c_active` bu fonksiyondan üretilir. `App.tsx` her TF için `buildElliottProjectionOverlayV2(..., sourceTf)` çağırır.

| Kural | Uygulama |
|--------|-----------|
| **Zaman / fiyat çapası** | Başlangıç fiyatı ve zamanı **`out.ohlcByTf[sourceTf]` son mumunun `c` / `t`** değeridir (yoksa `anchorRows` son mumu). Ana grafik interval’i farklı olsa bile projeksiyon ilgili TF serisine hizalanır. |
| **Dalga adımı seçimi** | `postImpulseAbc` varsa `startStepFromPostAbc`; yoksa P5’e göre `startStepFromCurrentState`. A/B teyidi yoksa düzeltme her zaman **A**’dan (`startStep` ≥ 4 zorlanmaz). |
| **Büyüklük** | `base`: itkı 1/3/5 ortalaması + son 14 mum ATR karışımı (`blendedProjectionPriceBase`). `buildCalibrationFromPostAbc` → `applyFormationCalibrationTweak`. Segment fiyat adımı ayrıca ATR tabanlı alt/üst sınırlarla sıkıştırılır (`atrFloor` clamp). |
| **Süre (zaman ekseni)** | Taban: `Δt ≈ (\|Δprice\| / stepRate) × fibZamanÇarpanı` — `projectionFibTimeMultiplier` klasik bant (0.382 / 0.618 / 1.618 …) ile ölçülen itkı bacak sürelerinin karışımı. `stepSec` (`barHop` × ortalama mum aralığı) bantları ile `clamp`. **Alt sınır:** en az **bir tam mum süresi** (`barPeriodSec`) ve `max(45s, 0.28×stepSec)`. |
| **İkinci senaryo** | `includeAltScenario` / `show_projection_alt_scenario`: `extendedThirdWaveCalibration` ile daha yüksek 3. dalga hedefi; `zigzagKind: elliott_projection_alt`, işaret sonu `※`; grafikte soluk renk (`scaleElliottHexColor`). |
| **Piyasa rejimi** | Son ~40 mumun ortalama fiyat/s hızı, itkı ortalama hızına göre **0.48–2.05** çarpanı (`regimeMul`); volatilite artınca süre kısalır. |
| **Etiketler** | `SeriesMarker` metni: dalga etiketi + başlangıca göre birikimli süre (`+Nm`, `+Nh`, `+Nd`). |

Ayarlar: `elliottWaveAppConfig` — `show_projection_*`, `show_projection_alt_scenario`, `projection_bar_hop`, `projection_steps`.

When behavior here changes, update this subsection in the same PR as code changes.

### 8.2 Forward projection — known limits and roadmap

Normative **§8.1** describes current behavior. The following gaps are intentional trade-offs or future work (audit alignment: `projection.ts`).

| ID | Limit | Notes |
|----|--------|--------|
| **E1** | Geometry | Output is a **polyline** of `{ time, value }` segments. Slope is implicit via `Δprice/Δtime` per leg (`RateProfile` + `stepRate`); there is **no** arc/channel geometry or separate “angular drawing” mode. |
| **E2** | Fib ratios | `DEFAULT_CALIBRATION` holds fixed multipliers (`aMul`/`bMul`/…); `buildCalibrationFromPostAbc` scales them from observed ABC **magnitudes**, not as explicit horizontal Fib **price levels** on chart. |
| **E3** | Formation-aware path | After impulse, the engine assumes an **A–B–C style** corrective cycle for step selection when `postImpulseAbc` exists; it does **not** branch on flat vs triangle vs W–X–Y. `startStepFromCurrentState` uses **price distance vs `base` bands**, not labeled wave position (e.g. “only wave 3 done → project 4–5”). |
| **E4** | Alternatives | **Kısmen:** birincil + `elliott_projection_alt` (uzatılmış 3. dalga); geniş “fan” (3+ senaryo) yok. |
| **E5** | Time Fib | **Kısmen:** `projectionFibTimeMultiplier` ile klasik oranlar + ölçülen süreler karışımı; tam Fib zaman projeksiyonu literatürü değil. |

**Roadmap (remaining):**

1. **Formation-aware selector** — Use last confirmed structure (impulse leg / corrective type) to choose the next template: e.g. post–wave-5 → ABC vs flat vs triangle priors; mid-impulse → project waves 4–5 only; post–ABC → new impulse.
2. ~~**Fibonacci time**~~ — İlk sürüm **§8.1** ile uygulandı; ince ayar ve isteğe bağlı kapama anahtarı ileride.
3. **Slope policy** — Keep polyline but enforce clearer **motive vs corrective** steepness ratio from measured history (beyond current `motiveRate` / `corrRate` median).
4. ~~**Multi-scenario (2 yol)**~~ — Birincil + alt senaryo **§8.1**; ek varyantlar (ör. kısa 5) isteğe bağlı.
5. **Horizontal Fib levels** — Optional marker or line layers at 0.382 / 0.618 / 1.0 / 1.618 / 2.618 of a chosen swing for targets, in addition to the projection polyline.

**Corrective ratios (`tezWaveChecks.ts`):** Flat filter uses `TEZ_FLAT_B_VS_A_MIN = 0.382` … `MAX = 2.618` so **expanded** flats are not dropped; `TEZ_CLASSIC_REGULAR_FLAT_B_MIN = 0.618` is documented for scoring/detail only — consistent with a deliberate “wide net” detection policy.

On-chart **hex colors** for macro/intermediate/micro are typically driven by app config (`web/src/lib/elliottWaveAppConfig.ts` and friends), not only this doc — keep **§3** and the config defaults in sync when you change the palette.

When behavior or labels change in code, update **§2–§5** here in the same PR when the change is intentional; if code diverges by mistake, treat this file as the checklist to reconcile (see `QTSS_MASTER_DEV_GUIDE.md` §1.3 **L4**).
