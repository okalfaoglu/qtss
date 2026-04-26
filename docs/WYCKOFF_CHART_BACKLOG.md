# Wyckoff / Elliott Chart Visualisation — Bug Backlog

User-flagged visual bugs from multi-AI chart audits (Gemini, Claude self-audit,
post-FAZ 25.4.G state). User instruction: "büyük proje bitince bakarsın" —
file these for after the FAZ 26 backtest module ships.

## ETH 15m chart (A-E box only, post FAZ 25.4.G)

### Working correctly
- Range Accum box (~2310-2325) at the right time, Spring · PE at the lower
  liquidity-grab — textbook Phase C
- Range Dist box (~2335-2348) above the markup, UTAD · PE at the upper test
- N (new HH/LL) and F= (failed break) markers in sensible positions
- Phase flow Spring → Range Accum → markup → Range Dist → UTAD canonical

### Open bugs

**B-CTX-1 — orphan Spring/UTAD events from off-screen ranges.**
"Spring · PE" at the far-left price ~2300 has no Accumulation box visible
on screen. Same for the "UTAD · PE" near 2330 above the Accumulation. The
bar tape extends back further than the visible chart window; events that
belong to OFF-SCREEN ranges still render, leaving floating labels.
*Fix idea*: gate event rendering to require the parent range tile to be
within the visible time window (or render the parent range box even if
its centre is off-screen).

**B-CTX-2 — Range Dist closes too early when price keeps rising.**
A Distribution range should close when a confirmed breakdown happens. The
current detector closes the box at the latest UTAD bar regardless of
whether price has actually broken down. After that close, price kept
rising on the chart — visually the box ended too soon.
*Fix idea*: keep the range tile `completed=false` (open border) until a
breakdown event (SOW + close < range_low) fires; only then snap end_bar
to that breakdown.

**B-CTX-3 — UTAD that fails (price keeps rising) should be re-classified.**
The right-edge "UTAD · PE" was followed by a 2350+ rally, not the expected
markdown. Per Wyckoff doctrine this is a FAILED UTAD (a.k.a. "minor
upthrust"). Need a post-confirmation check: if N bars after UTAD price
has not closed below the range low, re-label as `upthrust_failed_bear`
(or strip the label).

**B-CTX-4 — too many motives at 15m (7 in 1-2 days).**
Tight bar windows produce too many overlapping motives at the smallest
Z slot. Per-era canonical pick is already in place at the writer; chart
side might need an additional time-based dedup ("only one motive per
N candle window per slot").

## BTC 4h chart — A-E box only vs all cycles

### Behaviour
With ONLY `A-E box` checked, only the active Range Dist · PD shows. Older
Distribution / Accumulation / Markup / Markdown tiles are filtered out
because they're cycle rows, not range rows.

### Open improvement

**B-CTX-5 — default chart preset should pre-check Cycles + Ranges.**
The user expected to see the full Distribution → Markdown → Accumulation
→ Markup rotation by default. Currently they need to manually check four
Cycles checkboxes plus A-E box. Default the Cycles set to ON when Wyckoff
master is toggled on.
*Fix*: in `LuxAlgoChart.tsx`, when `setShowWyckoff(true)` fires, also
ensure `wyckoffFilter.cycle_*` are all true (already default true; the
visual confusion was the user expected Cycles to be implicit when the
master Wyckoff was on).

## BTC 4h chart — all cycles enabled

### Working correctly
- Distribution → Markdown → Accumulation → Markup → Distribution full flow
- Elliott motive (1)-(2)-(3)-(4)-(5) + ABC (a)-(b)-(c) labels everywhere
- Spring · PC under the Accumulation low, UTAD · PC at the Distribution top

### Open bugs

**B-CTX-6 — Markdown box overlaps Distribution box (left edge bleed).**
The big Markdown · Z3 tile's left edge starts at the Distribution box
boundary, so the two cycle phases share a few bars of overlap. Each phase
should get a clean exclusive window.
*Fix*: in `cycle.rs::detect_cycles_from_elliott`, when emitting a
Markdown that follows a Distribution, snap Markdown.start_bar = max(
Markdown.start_bar, Distribution.end_bar + 1).

**B-CTX-7 — micro Accumulation tiles span only a few bars.**
The (a)-(b)-(c) reaction range got tagged as a 7-day Accumulation. Real
Wyckoff Accumulation usually requires several weeks. Add a minimum
duration gate (e.g. `cycle_alignment.min_bars` per timeframe) to skip
sub-threshold tiles or downgrade them to "reaction range" labels.

**B-CTX-8 — label crowding on the right edge.**
Accumulation Z3 ◆, Markup Z3 ◆, Range Dist PD, Distribution Z3 ★, UTAD PC,
Spring PC, (1)(2)(4)(5)(a)(b)(c) all stack inside ~12 candles at the
right edge. Need:
  - Vertical offset stacking (each label nudged ±N pixels per existing
    overlap)
  - Or: collapse identical-position labels into a single combined chip
    "Markup ◆ + (5)" type joint label.

**B-CTX-9 — ★ vs ◆ glyph difference undocumented in the UI.**
Right-side Distribution has ★ (confluent), left-side Distribution has ◆
(elliott-only). Without a legend pop-up the operator can't tell what the
glyphs mean. Add a tooltip or legend chip.

## Cycle phase mapping discrepancy
**B-CTX-10 — sub-wave Accumulation [W0, W2] wrongly absorbs trend-leg
range when motive direction-detection misfires.**
Earlier audit showed an Accumulation tile spanning the May-Aug 2025 BTC
rally on Z5 1d (the actual rally was Markup but Z5 motive was missing
the bullish 5-wave detection). Already documented in
`ELLIOTT_WYCKOFF_INTEGRATION.md` §VII.6 edge cases. Fix lands when the
Pine port motive detector gets the missing pivot at L4 (FAZ 25.4.H —
true Elliott degree stratification).

---

Tracker: revisit each B-CTX-* item after FAZ 26 backtest module ships.
