# Elliott V2 Standards

This document defines naming, coloring, wave labels, hard rules, and ZigZag scope for the Elliott V2 engine.

For a broader **Rules vs Guidelines** checklist and a **code mapping / gap** section (including why corrective-only menu selections may not draw wave 2/4), see [`ELLIOTT_WAVE_RULES_REFERENCE.md`](./ELLIOTT_WAVE_RULES_REFERENCE.md).

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
3. **Standard** impulse: wave 4 cannot overlap wave 1 price territory (`impulse.ts` — `w4_no_overlap_w1`). **Diagonal** impulse is a separate variant: overlap allowed; extra rules (`ed_r4_w3_area_gt_w2`, `w5_not_longest_135`, `ld_r3_w5_ge_1382_w4` — **ld_r3 is applied only when `diagonalRole` is `leading`** after `mapLdR3ByDiagonalRole` in `engine.ts`, `w4_diagonal_mode`). Diagonal motive is **on by default** in the pattern menu (`motive_diagonal_leading` and `motive_diagonal_ending` in `elliottPatternMenuCatalog.ts`); users may disable either role.

### Diagonal engine behavior (V2)

`ImpulseCountV2.diagonalRole` (`leading` | `ending` | `unknown`) is chosen from (1) a **chart-window heuristic** (`inferDiagonalRoleFromChart` — early vs late placement on the zigzag pivot list), and/or (2) **MTF parent overlap** when the second pass applies (see below). `ImpulseCountV2.diagonalRoleSource` is `chart_window` or `mtf_parent`. **`unknown` uses the same nested proof path as ending** (3-3-3-3-3-style nested ABC checks per leg). **Leading** uses a 5-3-5-3-5-style path (nested motive on legs 1/3/5, nested corrective on 2/4). Informational check ids: `diagonal_interpret_ending` (`impulse.ts`), `diagonal_role_hint` (`engine.ts`).

### MTF-based `diagonalRole` — v1 (implemented)

**Goal:** Classify a **micro-TF diagonal** (`15m` or `1h`) as **leading** vs **ending** using **parent motive wave-1 vs wave-5** time overlap (first scope: **W1 vs W5** only, not corrective **C**).

**Mechanism:** After all TFs are computed, `applyMtfDiagonalRefinement` (`engine.ts`) runs. It compares the diagonal’s pivot **epoch** span `[min(p0.time,p5.time), max(...)]` to the parent impulse’s **W1** (`p0–p1`) and **W5** (`p4–p5`) spans. Overlap fractions use constants in `diagonalMtf.ts` (`MIN_COVERAGE`, `MARGIN`). **Precedence:** for `15m`, try **`1h`** parent first, then **`4h`**; for `1h`, use **`4h`** only. Parent states with `decision === invalid`** are skipped (no MTF opinion). When MTF yields a clear **leading** or **ending**, the engine **rebuilds** nested diagonal checks via `buildDiagonalSubwaveProof` with `diagonalRoleSource: mtf_parent` and refreshes `decision`.

**Limits:** Wrong parent impulse → wrong role (still a heuristic). **Wave 3** / complex contexts → often **no** MTF hit (chart-only role). **Ending diagonal in C** (not W5) is **not** in v1.

### Corrective scope

- Enabled: `ABC`, `zigzag (5-3-5)`, `flat (3-3-5)`, `triangle (3-3-3-3-3)`, `W-X-Y / W-X-Y-X-Z`.
- Notes:
  - Triangle detection is constrained to wave-4 / post-impulse corrective context (not standalone wave-2 default).
  - Complex combinations are scored as candidates/confirmed via ratio + connector checks; **W–X–Y** candidates also require **`comb_r8`** (at most one zigzag-like segment in the **Y** portion) and **`comb_r9`** (if **Y** is long enough, do not accept a triangle-like fit on the **first** six Y pivots when the **last** six Y pivots do not fit the same triangle geometry — triangle expected toward the end of **Y**).
  - Diagonal motive is optional (menu toggle); both standard and diagonal can be enabled together.
  - **Chart overlay:** post-impulse ABC (`+…`) uses dedicated `zigzagKind` **`elliott_v2_post_abc`** so **Dashed** is enforced in `TvChartPane` (not overridden by micro impulse styling). If the engine stores a long micro `path`, the **line** uses **corner `pivots` only** when shorter (`adapter.ts`).

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

**`confirmed` (per timeframe state)** — in addition to impulse hard rules, the engine requires **both** wave 2 and wave 4 correctives to satisfy `correctiveIsConfirmed` when those legs are present (`engine.ts`). That includes nested-structure checks where implemented (e.g. zigzag `zz_*_motive5`, flat `flat_*`, triangle `tri_*_corrective3`, combination `comb_confirmed` / `wxyxz_confirmed`). **Diagonal** impulses additionally require the nested diagonal leg checks (`diag_*_corrective3` or `diag_ld_*` by role). A **failed fifth** caps **`confirmed`** unless nested wave-5 **standard** motive evidence exists (`wave5NestedImpulse` with `variant: standard`; inner diagonal does not qualify). Optional nested overlays (`wave1NestedImpulse`, …) remain **informational** for UI and do not by themselves gate `confirmed`.

## 8) Implementation map (web — drift control)

This document is the **normative** label/color/rule set. The running engine lives under `web/src/lib/elliottEngineV2/`:

| Topic | Primary code |
|-------|----------------|
| Timeframe type + MTF bucketing (`15m` anchor → `15m`+`1h`+`4h`, …) | `types.ts`, `mtf.ts` |
| Impulse / corrective wave logic | `impulse.ts`, `corrective.ts` |
| Hard checks (compression, overlap-style guards) | `tezWaveChecks.ts`, `engine.ts` |
| ZigZag-driven pivots | `zigzag.ts` |
| Chart overlays: labels, line styles, per-TF coloring hooks | `adapter.ts` (label glyphs per TF; colors often supplied via `elliottWaveAppConfig` / chart layer) |
| Engine orchestration, state merge across TFs, `confirmed` gates, diagonal role + nested proofs | `engine.ts`, `diagonalMtf.ts` — MTF `diagonalRole` refinement **§4** |
| **Forward projection (İleri Fib / Elliott projeksiyon)** | `projection.ts` — `buildElliottProjectionOverlayV2` — behavior **§8.1**, limits / roadmap **§8.2** |
| Public exports | `index.ts` |

### 8.1 Forward projection (`projection.ts`) — normative behavior

Yalnızca **formasyon tabanlı** ileri çizim: `buildFormationProjection` (ABC / sonraki 1–5 segmentleri, yatay hedef çizgileri). `postImpulseAbc` teyitli kısım için `elliott_projection_done` / `elliott_projection_c_active`. Çoklu düzeltme renkleri: `projection_multi_corrective_scenarios`. `App.tsx` her TF için `buildElliottProjectionOverlayV2(..., sourceTf)` çağırır.

| Kural | Uygulama |
|--------|-----------|
| **Zaman / fiyat çapası** | Başlangıç fiyatı ve zamanı **`out.ohlcByTf[sourceTf]` son mumunun `c` / `t`** (yoksa `anchorRows` son mumu). |
| **Ana geometri** | `buildFormationProjection`: itkı ölçeği + örnek Fib oranlarıyla A–B–C ve tamamlandıktan sonra yeni itki segmentleri; süreler itkı bacak sürelerinden türetilen `durationA/B/C`. |
| **C ilerlemesi** | `blendedProjectionPriceBase` yalnızca `postImpulseAbc` yolunda **C tamamlandı mı** ayrımı (`cCompleted`) için kullanılır. |

Ayarlar: `elliottWaveAppConfig` — `show_projection_*`, `projection_multi_corrective_scenarios`.

When behavior here changes, update this subsection in the same PR as code changes.

### 8.2 Forward projection — known limits and roadmap

Normative **§8.1** describes current behavior. The following gaps are intentional trade-offs or future work (audit alignment: `projection.ts`).

| ID | Limit | Notes |
|----|--------|--------|
| **E1** | Geometry | Çıktı **segment polyline** + yatay hedef hatları; ayrı yay/kanal geometrisi yok. |
| **E2** | Fib ratios | Formasyon projeksiyonu sabit örnek oranlar kullanır; tam Fib seviye grid’i değil. |
| **E3** | Formation-aware path | Motor `postImpulseAbc` kalıbına göre çizim katmanı üretir; düzeltme dalgası türüne göre otomatik dallanma sınırlı. |
| **E4** | Alternatives | `projection_multi_corrective_scenarios` ile zigzag/yassı renk ayrımı (ABC adayları). |
| **E5** | Time Fib | Süreler itkı bacak sürelerinden ölçeklenir; literatür “Fib zaman” projeksiyonu değil. |

**Roadmap (remaining):**

1. **Formation-aware selector** — Use last confirmed structure (impulse leg / corrective type) to choose the next template: e.g. post–wave-5 → ABC vs flat vs triangle priors; mid-impulse → project waves 4–5 only; post–ABC → new impulse.
2. **Fibonacci time** — İsteğe bağlı ince ayar / kapama anahtarı ileride.
3. **Slope policy** — Segment eğimlerini ölçülen tarihsel itkı/düzeltme hızlarına daha sıkı bağlama.
4. **Ek projeksiyon varyantları** — Örn. kısa 5 / alternatif kalibrasyon; kural tabanlı tanım gerektirir.
5. **Horizontal Fib levels** — Optional marker or line layers at 0.382 / 0.618 / 1.0 / 1.618 / 2.618 of a chosen swing for targets, in addition to the projection polyline.

**Corrective ratios (`tezWaveChecks.ts`):** Flat filter uses `TEZ_FLAT_B_VS_A_MIN = 0.382` … `MAX = 2.618` so **expanded** flats are not dropped; `TEZ_CLASSIC_REGULAR_FLAT_B_MIN = 0.618` is documented for scoring/detail only — consistent with a deliberate “wide net” detection policy.

On-chart **hex colors** for macro/intermediate/micro are typically driven by app config (`web/src/lib/elliottWaveAppConfig.ts` and friends), not only this doc — keep **§3** and the config defaults in sync when you change the palette.

When behavior or labels change in code, update **§2–§5** here in the same PR when the change is intentional; if code diverges by mistake, treat this file as the checklist to reconcile (see `QTSS_MASTER_DEV_GUIDE.md` §1.3 **L4**).
