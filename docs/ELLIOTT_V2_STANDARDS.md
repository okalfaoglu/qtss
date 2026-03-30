# Elliott V2 Standards

This document defines naming, coloring, wave labels, hard rules, and ZigZag scope for the Elliott V2 engine.

## 1) Timeframe Hierarchy

- Macro: `4h`
- Intermediate: `1h`
- Micro: `15m`

The chart anchor is `15m`. `1h` and `4h` are rendered as overlays on the same chart.

## 2) Naming and Labeling

- `4h` labels:
  - Impulse: `1 2 3 4 5`
  - Corrective: `A B C`
- `1h` labels:
  - Impulse: `(1) (2) (3) (4) (5)`
  - Corrective: `(A) (B) (C)`
- `15m` labels:
  - Impulse: `i ii iii iv v`
  - Corrective: `a b c`

## 3) Color Standard

- Macro `4h`: `#1E88E5` (blue)
- Intermediate `1h`: `#43A047` (green)
- Micro `15m`: `#FB8C00` (orange)
- Invalidation markers: `#E53935` (red)
- Neutral/assistive hints: `#B0BEC5`

## 4) Elliott Rule Scope (V2.0)

### Hard invalidation rules for impulse

1. Wave 2 cannot retrace beyond the start of Wave 1.
2. Wave 3 cannot be the shortest among Waves 1, 3, 5.
3. Wave 4 cannot overlap Wave 1 price territory (diagonal exception is not enabled in V2.0).

### Corrective scope

- Enabled: `ABC`, `zigzag (5-3-5)`, `flat (3-3-5)`, `triangle (3-3-3-3-3)`, `W-X-Y / W-X-Y-X-Z`.
- Notes:
  - Triangle detection is constrained to wave-4 / post-impulse corrective context (not standalone wave-2 default).
  - Complex combinations are scored as candidates/confirmed via ratio + connector checks.
  - Advanced diagonal handling remains optional and is disabled by default in V2.

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
| **Forward projection (İleri Fib / Elliott projeksiyon)** | `projection.ts` — `buildElliottProjectionOverlayV2` |
| Public exports | `index.ts` |

### 8.1 Forward projection (`projection.ts`) — normative behavior

Chart overlay `zigzagKind: elliott_projection` (ve varsa `elliott_projection_done` / `elliott_projection_c_active`) bu fonksiyondan üretilir. `App.tsx` her TF için `buildElliottProjectionOverlayV2(..., sourceTf)` çağırır.

| Kural | Uygulama |
|--------|-----------|
| **Zaman / fiyat çapası** | Başlangıç fiyatı ve zamanı **`out.ohlcByTf[sourceTf]` son mumunun `c` / `t`** değeridir (yoksa `anchorRows` son mumu). Ana grafik interval’i farklı olsa bile projeksiyon ilgili TF serisine hizalanır. |
| **Dalga adımı seçimi** | `postImpulseAbc` varsa `startStepFromPostAbc`; yoksa P5’e göre `startStepFromCurrentState`. A/B teyidi yoksa düzeltme her zaman **A**’dan (`startStep` ≥ 4 zorlanmaz). |
| **Büyüklük** | İtkı bacak ortalaması `base`; `buildCalibrationFromPostAbc` ile A/B/C ve 1–5 çarpanları; `stepMagnitudeWithCalibration`. |
| **Süre (zaman ekseni)** | Her segment için `Δt ≈ \|Δprice\| / stepRate` (itkı veya düzeltme için pivotlardan türetilen fiyat/s), `stepSec` (`barHop` × ortalama mum aralığı) ile üst bant. **Alt sınır:** en az **bir tam mum süresi** (`barPeriodSec`) ve `max(45s, 0.28×stepSec)` — uzun geçmişte grafik ölçeğinde tüm adımların tek dikey çizgide birleşmesini önler. |
| **Piyasa rejimi** | Son ~40 mumun ortalama fiyat/s hızı, itkı ortalama hızına göre **0.48–2.05** çarpanı (`regimeMul`); volatilite artınca süre kısalır. |
| **Etiketler** | `SeriesMarker` metni: dalga etiketi + başlangıca göre birikimli süre (`+Nm`, `+Nh`, `+Nd`). |

Ayarlar: `elliottWaveAppConfig` — `show_projection_*`, `projection_bar_hop`, `projection_steps`.

When behavior here changes, update this subsection in the same PR as code changes.

On-chart **hex colors** for macro/intermediate/micro are typically driven by app config (`web/src/lib/elliottWaveAppConfig.ts` and friends), not only this doc — keep **§3** and the config defaults in sync when you change the palette.

When behavior or labels change in code, update **§2–§5** here in the same PR when the change is intentional; if code diverges by mistake, treat this file as the checklist to reconcile (see `QTSS_MASTER_DEV_GUIDE.md` §1.3 **L4**).
