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

## ChatGPT meta-audit (BTC 4h Z3, all cycles + ranges + events on)

ChatGPT scored the chart as "%60 doğru yapı, %40 aşırı yorum/erken
etiketleme". Three new categories beyond what the earlier AIs caught:

**B-CTX-11 — over-labeling: "her şeyi koymuş" feel.**
With Cycles + Ranges + Spring/UTAD + Climax + SOS/SOW + sources all
toggled on, the chart shows simultaneous Spring + UTAD + (1)-(5) +
range box + cycle tile labels in tight clusters. ChatGPT note: "İyi
analiz sade olur. Bu kadar sinyalin aynı anda 'mükemmel' çıkması
genelde gerçekçi değil."

This is more than just label-crowding (B-CTX-8) — it's a confidence
hierarchy issue. Fix idea: introduce a "high-conviction-only" mode
that suppresses anything below a confidence floor (e.g. only confluent
★ tiles + Pruden-aligned events). Make this the default; let power
users explicitly enable the full firehose.

**B-CTX-12 — UTAD label fires before rejection is confirmed.**
ChatGPT: "UTAD demek için öncesinde net bir distribution range +
üstten fake breakout + hızlı geri dönüş gerekir. Burada fiyat hâlâ
range içinde sayılır, net bir rejection yok. 'UTAD olmuş' demek
erken. 'UTAD ihtimali var' demek daha doğru."

Tighter constraint than B-CTX-3:
  - B-CTX-3 was about RETROACTIVELY downgrading a UTAD that was
    later invalidated by price action.
  - B-CTX-12 is about NOT firing the UTAD label until confirmation
    happens (provisional UTAD vs confirmed UTAD).

Fix: introduce a `provisional` flag on Spring/UTAD/SC/BC events.
Default `provisional=true` for the first N bars after fire; flip to
`confirmed=true` only after the rejection / breakdown is observed
(e.g. UTAD confirmed when price closes back inside the range AND
volume on the rejection bar > some threshold). Chart renders
provisional events with a different glyph (e.g. dashed border, "?"
suffix) so the operator sees uncertainty visually.

**B-CTX-13 — Elliott "zorla oturtulmuş" — complex correction
mis-classified as 5-wave impulse.**
ChatGPT: "(5) sonrası gelen yapı net impuls gibi değil, daha çok
karmaşık düzeltme (complex correction) gibi duruyor. Elliott burada
ikincil araç olmalı, ana değil."

The Pine port's motive detector classifies any 6-pivot sequence
satisfying basic ordering as a motive, but Elliott doctrine says when
a count "feels forced" it's often a corrective in disguise (W-X-Y,
W-X-Y-X-Z combination, expanded flat where (b) > prior peak, etc.).
The qtss-elliott crate already has detectors for these (see
elliott_full subkinds: combination_wxy_*, flat_running_*) but the
preference between motive vs combination relies only on which fired —
no scoring against each other.

Fix idea: ambiguity scorer. When BOTH a motive and a combination /
flat overlap on the same time window at the same Z slot, evaluate a
"complexity score" that downgrades the motive when (a) wave 4
overlaps wave 1 by > X% of wave 1 height, or (b) wave 3 is the
shortest, or (c) wave 5 truncation pattern matches a c-wave better
than a 5-wave. Pick the higher-scoring interpretation.

This is structurally a motive-detector improvement (qtss-elliott),
not a chart fix — track here for cross-reference.

## Operational signals (NOT bugs — feature ideas)

ChatGPT also surfaced three "what to watch" questions that the chart
could AUTOMATICALLY answer for the operator:

**B-CTX-FT-1 — break/no-break confirmation overlay.**
"78-80k üstü kabul → markup devam. Range içine sert geri dönüş →
UTAD senaryosu güçlenir. Hacim hacimsizse → fake olma ihtimali artar."

The system already knows the active Range Dist's high; it should
draw a "decision price" line + render a small overlay annotation
showing "X bars / Y volume above range high → confirmed breakout"
or "rejection at range high + volume → UTAD confirmed".

**B-CTX-FT-2 — confidence-tiered UI mode.**
A "Lite" preset that hides all but the highest-confidence elements:
  - Cycle tiles only when source = Confluent ★
  - Events only when Pruden-aligned (Spring + W2 / UTAD + B / etc.)
  - Range boxes only when phase advanced past Phase B
  - All Elliott (1)-(5) hidden, only ABC corrective + final motive top

**B-CTX-FT-3 — volume + momentum diagnostic at break/test points.**
At each Range high/low test, attach a small data tooltip showing:
  - Volume vs 20-bar SMA
  - Momentum (RSI / MACD histogram) divergence
  - CVD trend in the test window
This converts subjective "is this UTAD real?" into a visual checklist.

## Claude's major-minor distinction audit (POST-26.6 backlog)

User asked Claude to evaluate the system's ability to distinguish
MAJOR (structural) vs MINOR (reaction) dip/top points. Claude's
verdict: "Sistem pattern recognition yapıyor — yapıyı tanıyor. Major-
minor ayrımı için ise pattern qualification lazım — tanıdığı yapıyı
ölçmesi gerekiyor."

### Strengths Claude observed
  - (5) wave labels at structural dips/tops: clean detection.
  - Range-internal Spring/UTAD as minor entries: correctly typed.
  - (a)(b)(c) corrective separation from major impulse: clean.

### Weaknesses Claude observed
  - 99k structural top of Jan 2026 left unmarked (Distribution box
    7-10k below the actual peak — late detection).
  - All labels render at the SAME visual weight: (5) major and (4)
    minor look identical → user can't see hierarchy at-a-glance.
  - On busy bars (5+ events in a tight window), no priority to
    decide which is the actionable signal.
  - No volume confirmation — Wyckoff IS volume analysis at heart.

### Proposed enhancements (Claude's priority order)

**B-CTX-MM-1 (HIGHEST PRIORITY) — Volume integration**
"Wyckoff fiilen hacim analizidir — fiyat-hacim ilişkisi olmadan
Wyckoff yarı-Wyckoff'tur." Concrete asks:
  - Effort vs Result: high-volume Spring with reclaim → real Spring;
    low-volume → suspect (downgrade to "weak Spring" with reduced
    confidence).
  - Drying-up detection: Distribution Phase D should show shrinking
    volume; Accumulation Phase B same. Auto-flag "yapı sağlıklı /
    sağlıksız" when volume profile contradicts the phase claim.
  - SOS volume confirmation: range break-up bar volume > X×SMA →
    confirmed SOS; less → fakeout (reclassify, drop the label).
  - Detector-level: every Phase-C event evaluator should already
    have access to bar.volume; gate the score by volume_ratio
    threshold per event type.

**B-CTX-MM-2 — Multi-cycle confluence indicator**
Auto-detect when the SAME zone is signalled by multiple Z slots:
  - "Major Distribution Confluence" badge fires when Z5 Distribution
    + Z3 Distribution + Z4 UTAD all overlap a price band ≤ X% wide.
  - Primary-cycle selection: user picks one Z (e.g. Z5) as the
    "anchor"; lower Z labels render as "tepki / minor" automatically
    when their direction matches Z5's trend.
  - Contradiction warning: Z3 Distribution while Z5 in Accumulation
    → big yellow banner "cycle conflict — exercise caution".

**B-CTX-MM-3 — Visual hierarchy (size + color tiering)**
  - Major points (Z5 (5) wave, confluent ★): 150% font size,
    saturated colour.
  - Minor points (Z3 (4) wave, single-source ◆): 100%, muted.
  - Confidence score badge: every label gets a 1-5 chip ("UTAD · PC
    (4/5)"). Below threshold dims to ghost.

**B-CTX-MM-4 — Phase maturity / progression bar**
  - Each range tile shows a small progress bar inside it: "Phase A
    → B → C → D → E (currently 70% through D)".
  - Age guard: <X bars old = NOT classified as range (downgrade to
    "reaction range" or hide entirely).
  - Early/late warnings: "this accumulation is too young, wait" or
    "distribution at Phase E, breakdown imminent".

**B-CTX-MM-5 — Fractal level scoring (1-5)**
Direct answer to the major-vs-minor question:
  - Level 5: highest TF reversal (across multiple symbols correlated)
  - Level 4: confluent across ≥3 Z slots
  - Level 3: single Z, strong confidence ★
  - Level 2: structural sub-wave (W2, W4)
  - Level 1: noise (filtered by default)
  Auto-assigned per detection; chart hides Level 1 by default and
  upgrades the visual treatment for higher levels.
  Components: ATR-multiple of the swing (big swing = higher level),
  liquidity zone proximity (HH/LL / round numbers boost), confluence
  count.

**B-CTX-MM-6 — Historical performance overlay**
  - Per-event-type win rate from the backtest (FAZ 26 deliverable!):
    "Z3 Spring · PC has been right 64% over the last 6 months on
    BTC 4h" — chip shown next to the live label.
  - Faded "invalidated" labels: don't delete, gray-out + tooltip
    explaining why ("UTAD invalidated — closed 2% above range
    high"). Keeps the chart honest and operator-trainable.
  - "Detection latency" metric: how many bars after the actual
    extremum did the system fire its label? Surfaces the system's
    own latency character.

**B-CTX-MM-7 — Context-aware labelling**
  - Trend context: UTAD inside a strong uptrend ≠ UTAD inside a
    sideways range. Apply a context multiplier to confidence.
  - Liquidity zone proximity: events near previous HH/LL / round
    numbers (50k, 60k, 70k) get a confidence bonus.
  - Time context: weekend / pre-news / illiquid windows trigger an
    advisory ("low-confidence regime, reduce sizing").

**B-CTX-MM-8 — User feedback loop**
  - Right-click "this is a major reversal / this label is wrong"
    captures operator overrides into a feedback table.
  - Per-user noise filter: "no more than 3 UTAD labels per Z3
    cycle in any 6-hour window" prevents notification storm.
  - Long-term: feedback drives a learned confidence model — each
    event subkind gets a per-symbol/tf calibration multiplier.

### Implementation order Claude recommends
  1. **Volume integration** (huge effect, ~moderate work)
  2. **Multi-cycle confluence indicator** (uses existing infra)
  3. **Visual hierarchy** (low cost, high readability win)
  4. **Confidence score chip** per label
  5. **Phase progression bar** inside range tiles
  6. **Volume-failed label dimming** (depends on #1)

Track these as the second wave of Wyckoff polish — pick up after
FAZ 26.6 (GUI Backtest Studio) ships.

---

Tracker: revisit each B-CTX-* item after FAZ 26 backtest module ships.
