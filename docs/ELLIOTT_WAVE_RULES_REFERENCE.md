# Elliott Wave — Rules vs Guidelines (reference checklist)

**Purpose:** Structured reference for comparing **formal Rules** (must hold for a valid pattern) vs **Guidelines** (probabilistic; more alignment ⇒ higher confidence). This file is derived from a consolidated checklist supplied for the QTSS project; it is **not** a substitute for primary Elliott Wave literature (Frost & Prechter, etc.).

**Türkçe özet:** Kurallar ihlal edilirse sayım geçersiz sayılır; rehber maddeler ihlal edilse de yapı kalabilir. QTSS motorunda şu an çoğunlukla **sınırlı bir sert kural kümesi** (`impulse.ts` vb.) ve **tez / skor kontrolleri** (`tezWaveChecks.ts`, düzeltme kanıtları) vardır; aşağıdaki uzun listedeki her madde kodda bire bir yoktur — detay için bölüm *QTSS V2 — code mapping*.

---

## 1. Rules vs Guidelines (conceptual)

| | Rules | Guidelines |
|---|--------|------------|
| **Violation** | Pattern does not qualify as that Elliott pattern | Pattern may still qualify; lower rating / probability |
| **Engine use** | Typically hard-fail or `invalid` gates | Typically scoring, soft checks, or UI hints |

---

## 2. Impulse (motive, 1–2–3–4–5)

### 2.1 Impulse — Rules (checklist summary)

- Wave **1**: must be an **Impulse** or **Leading Diagonal**.
- Wave **2**: any corrective **except Triangle**; cannot retrace more than 100% of wave 1; minimum **20%** retrace of wave 1; max time **9×** wave 1.
- Wave **3**: must be an **Impulse**; longer than wave 2 in gross price; not shorter than **1/3** of wave 1; not more than **7×** wave 1; max time **7×** wave 1; joint failure constraints with wave 1 / wave 5 (5th shorter than 4th).
- Sub-wave size constraints on wave 2 vs internals of waves 1 and 3 (gross movement vs wave 2/4 of 1 and 3, **61.8%** floor vs each of four sub-waves).
- Wave **4**: any corrective; overlap rules for 1/2/4 (classic non-overlap; **15% / 2-day** caveat for leveraged instruments in source text); size vs internals of 3 and 5; vs wave 2 (>**1/3**, **<3×** gross); max time **2×** wave 3; joint failure with wave 3.
- Wave **5**: **Impulse** or **Ending Diagonal**; if longer than wave 3 then must be Impulse; move **>70%** of wave 4 (end-point rule); wave 3 not shortest vs 1 and 5 (price and %); truncation / failure coupling; max vs wave 3 in price and time (**6×**).

### 2.2 Impulse — Guidelines (summary)

- Typical proportions: wave 2 often zigzag family, **>30%** retrace of 1, often **<80%**, common **50% / 61.8%**; wave 3 often **1.5–3.5×** wave 1 range; wave 4 often **~38.2%** of 3; alternation 2 vs 4; extension and volume heuristics; wave 5 targets vs Fib multiples of wave 1 / 1–3 span.

*(Full prose from the source checklist can be re-attached in an appendix if needed.)*

---

## 3. ZigZag (A–B–C)

### 3.1 Rules (summary)

- **A**: Impulse or Leading Diagonal.
- **B**: corrective only; shorter than **A** in price; **≥20%** of A; time **≤10×** A.
- **C**: Impulse or Ending Diagonal; if A is LD then C not ED; **C > 90%** of B; **C < 5×** B; failure coupling A vs C; **C ≤ 10×** A or B in price and time.

### 3.2 Guidelines (summary)

- B retracements (38.2 / 50 / 61.8%), time vs A, C length vs A (61.8 / 161.8), rare C slightly shorter than B, slope / extension notes.

---

## 4. Flat (A–B–C, sideways)

### 4.1 Rules (summary)

- **A**, **B**: correctives except Triangle; **B > 70%** retrace of A; **B < 2×** movement of A; **B** time **<10×** A.
- **C**: Impulse or ED; shares price territory with A; **C < 2×** max(A,B); **C < 3×** price distance of A; no back-to-back failures; **C ≤ 10×** A or B in price and time.

### 4.2 Guidelines (summary)

- A/B usually zigzag family; B often 95–140% of A; C length relations; time ratios.

---

## 5. Diagonal (LD / ED)

Rules cover internal structure (zigzag vs impulse per leg), overlap of 2–4, channel convergence, wave 5 vs 1/3 length, 5 vs 4 (**≥80%**), intersection beyond pattern, max intersection distance, wave 5 time vs 3/4.

Guidelines: typical zigzag legs, 35% retraces, overlap of 1–4 endpoints, time ratios.

---

## 6. Triangle (CT / ET)

Rules cover leg types, **50%** retraces (CT B vs A), channel bounds (**10%** beyond A–C / B–D), D/E constraints, E in territory of A, channel intersection ordering, max times, horizontal-line constraints.

Guidelines: usual zigzag legs, 61.8% relations, E vs D retrace (~70% CT).

---

## 7. Double / Triple ZigZag (W–X–Y, W–X–Y–X–Z)

Rules: W,Y,Z zigzags; X, XX correctives (not ET); size and time ratios between legs; no double failures; C of W/Y not failure where required.

Guidelines: X/XX depth and time vs W/Y, Y/Z equality to Fib multiples.

---

## 8. Double / Triple Sideways (flat family combinations)

Rules: W/X/Y/XX/Z pattern-type constraints; **70%** minimum retracements for X and XX; max **150%** span rules; Y/Z not zigzag if prior leg zigzag (per source); failure rules.

Guidelines: typical lengths 95–140%, 110% retraces, time windows.

---

## QTSS V2 — code mapping and gaps

Use this section when auditing the implementation against the checklist above.

### A. Where rules are enforced today

| Area | Location (approx.) | Role |
|------|-------------------|------|
| Impulse structure & core inequalities | `web/src/lib/elliottEngineV2/impulse.ts` (`checksBull` / `checksBear`, scoring) | Pivot-window candidates; ids such as `structure`, `w2_not_beyond_w1_start`, `w3_not_shortest_135`, `w4_no_overlap_w1`, extension / diagonal checks |
| Corrective pattern “tez” / proof checks | `web/src/lib/elliottEngineV2/tezWaveChecks.ts`, `corrective.ts` | Zigzag / flat / triangle proofs attached to detected correctives |
| State confidence | `web/src/lib/elliottEngineV2/engine.ts` (`decideTimeframeState`) | `invalid` / `candidate` / `confirmed` from hard fails + internal corrections |
| Chart polylines | `web/src/lib/elliottEngineV2/adapter.ts` (`v2ToChartOverlays`) | Maps engine state to drawable layers |

### B. UI / motor coupling (why “only corrective” may draw nothing)

1. **Dalga 2 / 4 / +ABC** are computed only **after** a candidate **impulse** is found: `detectImpulseCorrectionsV2(pivots, impulse, menu)` in `engine.ts`. If no impulse is accepted, `wave2`, `wave4`, `postImpulseAbc` stay `null`.
2. **Impulse detection** respects the pattern menu: `impulseDetectOptsFromMenu` → if **Normal Impulse** and **both diagonals** are off, `detectBestImpulseV2` typically finds **no** standard/diagonal impulse → `impulse === null` → **no** corrective legs tied to 1–5.
3. **Chart drawing** of the main 1–5 polyline and its wave2/4/post layers is additionally gated by `showImpulseOverlay` in `adapter.ts` (menu must allow the detected variant).

So: **checking only Zigzag / Flat / Triangle under “Corrective” does not enable standalone ABC search on the whole chart**; correctives are **nested in the impulse framework** in the current design.

4. **Ham ZigZag (pivot polyline)** is independent: `buildZigzagPivotsV2` + `zigMap` in `adapter.ts`. It should still draw when **ZigZag (pivot)** is on for that TF, **if** the timeframe has OHLC in `byTimeframe` and enough pivots after depth settings.

### C. Known gaps vs the full checklist

Many items in sections 2–8 above (time multiples 9×/7×/2×, leveraged overlap exceptions, joint failure matrices, full triangle channel algebra, triple-combination rules, etc.) are **not** implemented as exhaustive hard rules in QTSS. Treat the engine as a **partial** formalization plus heuristics; use this document to drive a phased backlog (hard rules vs guideline scoring).

### D. Chart layer merge (App)

`App.tsx` merges ACP pattern layers with Elliott layers up to a **cap** (40). If many ACP layers exist, Elliott layers could previously be dropped entirely; order was **ACP first**. When Elliott contributes layers, **Elliott is merged first** so pivots/waves are not silently truncated (see code comments in `mergedPatternLayers`).

---

## Revision

Update this file when adding major rule coverage in the engine or changing UI semantics (e.g. standalone corrective search).
