# Elliott Wave + Wyckoff Method — Theoretical & Practical Integration

User: "Elliott Dalga Teorisi ve Wyckoff Metodu arasındaki teorik ve
pratik ilişkiyi derinlemesine araştır, bu iki yaklaşımı birleştirerek
projeye nasıl entegre ederiz detaylandır. bu geliştirme itki ve
düzeltme tahminlemeyi ve bu tahminleme ile işlem açma üzerinde çalış."

This doc lays out the theoretical bridge, the empirical correspondence
between Wyckoff phases/events and Elliott waves, and a concrete
implementation roadmap for QTSS.

---

## I — Theoretical foundations

### I.1 Elliott Wave (Frost & Prechter, 1978)

**Premise**: market price moves in repeating fractal patterns driven
by mass psychology cycles. Two move types:

- **Impulse** (motive): 5-wave move in trend direction. Sub-rule:
  W1-W3-W5 are themselves 5-wave; W2-W4 are 3-wave correctives.
- **Corrective**: 3-wave (sometimes 5 in triangle) counter-trend
  move. Subtypes: zigzag (5-3-5), flat (3-3-5 reg/exp/run),
  triangle (3-3-3-3-3), combination (W-X-Y / W-X-Y-X-Z).

**Cardinal rules**:
1. W2 cannot retrace beyond W1 origin.
2. W3 cannot be the shortest of W1/W3/W5.
3. W4 cannot enter W1 territory.

**Strengths**: predictive (projects future targets); fractal (works
across timeframes); structural (rules are objective).

**Weaknesses**: subjective wave counting; many "valid" interpretations
at any given moment; insensitive to volume / order flow.

### I.2 Wyckoff Method (Richard D. Wyckoff, 1908-1934; refined Tom Williams 1990s)

**Premise**: the "Composite Operator" (smart money) accumulates and
distributes via discrete, observable phases visible in the
**relationship between price action and volume**. Markets are
controlled by professional money that leaves footprints.

**Five-phase schematic**:

| Phase | Accumulation | Distribution |
|---|---|---|
| **A** — stop the trend | PS → SC → AR → ST | PSY → BC → AR → ST |
| **B** — build cause | tests within range | tests within range |
| **C** — final test | Spring (false break low) | UTAD (upthrust above) |
| **D** — confirm | SOS → LPS → JAC | SOW → LPSY → JAC |
| **E** — markup / markdown | impulsive trend departure | impulsive trend departure |

**Core events** (each has price+volume+range fingerprint):

- **PS** (Preliminary Support): first volume spike on the way down
- **SC** (Selling Climax): panic capitulation, max volume + wide
  range + lower-shadow rejection
- **AR** (Automatic Rally): bounce after SC, defines range top
- **ST** (Secondary Test): low-volume retest of SC, taps support
- **Spring** (Phase C accum): false breakdown below SC + immediate
  reclaim — traps shorts
- **Test of Spring**: low-volume retest of spring low
- **SOS** (Sign of Strength): high-volume bullish impulse breaking
  range — Phase D launch
- **LPS** (Last Point of Support): higher-low after SOS
- **BU/JAC** (Back-Up / Jump-Across-Creek): breakout + pullback
  from range high
- **BC** (Buying Climax): distribution mirror of SC
- **UTAD** (Upthrust After Distribution): false breakout above
  range high — traps longs
- **SOW** (Sign of Weakness): high-volume bearish impulse breaking
  range support

**Strengths**: volume/order-flow grounded; identifies institutional
activity; precise event signatures.

**Weaknesses**: ranges hard to define mechanically; phase boundaries
fuzzy; doesn't predict targets directly (relies on Point & Figure
counts).

### I.3 Why they're complementary

| Question | Elliott | Wyckoff |
|---|---|---|
| What pattern is in progress? | wave count (W1..W5, A-B-C) | phase (A/B/C/D/E) |
| Where does it end? | Fib targets / structural rules | Point & Figure projection |
| Why does it move? | mass psychology fractal | smart-money supply/demand |
| Volume relevance? | secondary | **primary** |
| Time precision? | weak | strong (events have precise bars) |
| Signal density? | sparse (waves take time) | dense (events fire often) |

Elliott tells you the **shape of the move** (5 up / 3 down / which
wave we're in); Wyckoff tells you the **conviction behind it**
(smart money buying or selling, exhaustion or accumulation). A
wave count without volume confirmation is a guess; a Wyckoff
event without structural context is noise.

---

## II — Empirical mapping (Elliott waves ↔ Wyckoff phases)

The mapping is well-documented in the technical analysis literature
(David Weis, Hank Pruden, Bruno DiGiorgi). Each Elliott wave has a
characteristic Wyckoff phase footprint.

### II.1 Bullish cycle (full mapping)

```
Elliott:    [---- prior down trend ----][--- new impulse ---][---- correction ----]
            |                          ||                   ||                    |
Wyckoff:    | Phase A  | Phase B | Phase C | Phase D | Phase E | distribution starts
            |          |         |         |         |         |
Events:     PS SC AR ST | (range) | Spring  | SOS LPS | impulse |
```

**Wave-by-wave**:

| Elliott | Wyckoff phase | Wyckoff events expected | Confidence boost when present |
|---|---|---|---|
| End of prior bearish W5 (or C of correction) | Phase A start | PS, SC, AR | YES — capitulation = end of selling |
| W2 of new bull impulse | Phase B (testing) → Phase C | Secondary Test, then Spring | **VERY HIGH** — Spring + low W2 = textbook entry |
| Start of W3 | Phase D | SOS (sign of strength) | **VERY HIGH** — volume expansion confirms W3 |
| W3 mid-extension | Phase E (markup) | continuation, no events | low signal |
| W4 retracement | (often a mini Phase B/C inside a higher-degree range) | LPS (last point of support) | medium — LPS = W4 low candidate |
| W5 thrust | continued markup | (Phase A of distribution beginning?) | weak |
| W5 top / start of A-B-C correction | Phase A of distribution | BC (buying climax) | **VERY HIGH** — BC = W5 exhaustion |
| B wave of A-B-C | Phase B distribution / Phase C | UTAD | HIGH — UTAD + B-wave fakeout = entry SHORT |
| C wave of A-B-C | Phase D distribution | SOW (sign of weakness) | HIGH |

### II.2 Specific high-conviction setups

**Spring + W2 confluence** (BULLISH ENTRY)
- Wyckoff Spring fires (false break below range low + immediate reclaim)
- Elliott structure says current_wave is W2 (post-W1)
- Combined: smart money shook out late shorts at the W2 low → W3 launch is imminent
- Action: long entry, SL below Spring low, TP at W3 projection (W1 + 1.618×leg)

**SOS + W3 launch** (BULLISH CONTINUATION)
- Wyckoff SOS fires (high-volume break above range high)
- Elliott structure says current_wave was W2-completed, now entering W3
- Combined: institutional buying confirms W3 ignition
- Action: long entry on first pullback (LPS), SL at SOS origin, TP at W5 projection

**BC + W5 top** (BEARISH ENTRY)
- Wyckoff BC fires (high-volume blowoff with upper-shadow rejection)
- Elliott structure says current_wave is W5-completed
- Combined: smart money distributing at the structural top
- Action: short entry, SL above BC high, TP at A-wave projection

**UTAD + B-wave fakeout** (BEARISH CONTINUATION)
- Wyckoff UTAD fires (false break above prior high + reclaim down)
- Elliott structure says current_wave is B (of A-B-C correction in bear motive)
- Combined: B-wave fakeout traps late longs → C-wave plunge is next
- Action: short entry, SL above UTAD high, TP at C-wave projection (= A length)

**SOW + C-wave conclusion** (BULLISH REVERSAL)
- Wyckoff SOW fires (last bearish impulse) but iq_structures says current_wave is C
- Combined: capitulation at C-wave end → next move is the new motive's W1
- Action: long entry on next higher-low, SL at C low, TP at next motive's W3

### II.3 Conflict cases (when they disagree)

The signals are most powerful when both align; misalignment is also
informative — it's a flag to **pause** rather than trade either signal
alone.

| Elliott says | Wyckoff says | Interpretation |
|---|---|---|
| W3 in progress | Phase A (capitulation) | Conflict — wave count likely wrong; reset |
| W5 top | Phase D (markup continuation) | Wave count premature; W5 not done yet |
| C of correction | Spring | Possible early reversal — but require more confirmation |
| A of correction | UTAD | Top likely real, not a healthy correction |

---

## III — QTSS current state audit

### III.1 What's already in place

✅ **`qtss-wyckoff` crate** — 12-event detector + Phase A-E state
   machine (`WyckoffPhaseTracker`). 12 specs in `WYCKOFF_SPECS`
   (CLAUDE.md #1 dispatch table compliance).

✅ **`qtss-engine::writers::wyckoff`** — engine writer persists each
   event to `detections` with `pattern_family='wyckoff'`,
   `subkind=<event>_<variant>`, `raw_meta.phase` carries A-E.

✅ **Live data**: 818 rows across 6 event types (sc_bull,
   bc_bear, sos_bull, sow_bear, spring_bull, utad_bear).

✅ **`iq_structures`** — Elliott wave state machine per (sym, tf, slot).

✅ **`major_dip_candidates` / `major_top_candidates`** — composite
   scorers including a `structural_completion` channel that reads
   Elliott but NOT Wyckoff.

### III.2 What's missing (the integration gap)

❌ Wyckoff events live in `detections` but are NEVER cross-referenced
   with `iq_structures` rows.

❌ Major Dip composite has no `wyckoff_alignment` channel.

❌ IQ-D / IQ-T candidate loops have no Wyckoff gate.

❌ Analiz page doesn't surface Wyckoff phase per (sym, tf).

❌ Confluence engine (`qtss-analysis`) doesn't combine wyckoff +
   elliott_early into a single confluence score.

---

## IV — Integration architecture

### IV.1 Three layers of integration

```
Layer 1 — Per-component scoring (existing channel + new channel):
   structural_completion        (Elliott — already real)
   wyckoff_alignment            (NEW — Wyckoff phase ↔ Elliott wave check)

Layer 2 — Setup gating (boolean alignment requirement):
   iq_d_candidate spawns ONLY when:
     - iq_structure.current_wave ∈ {W1, W2, W3} AND
     - latest Wyckoff event for symbol/tf is compatible:
         W2 entry tier   → require Spring or Test-of-Spring within last N bars
         W3 entry tier   → require SOS within last N bars
         OR no Wyckoff data → fall through (don't block)
   iq_t_candidate similar with parent's correction phase:
     parent in B → require UTAD (for bear) or no contrary signal
     parent in C → require SOW (for bear) / SOS (for bull) imminent

Layer 3 — Confidence amplifier (multiplier on Major Dip score):
   when Wyckoff event aligns with Elliott projection, boost composite
   score by 1.2× (capped at 1.0). Trader can see "structurally clean
   AND volume-confirmed" rows surface to the top of Analiz.
```

### IV.2 The new `wyckoff_alignment` scorer (Layer 1)

```rust
fn score_wyckoff_alignment(
    pool: &PgPool,
    s: &SymbolKey,
    polarity: Polarity,
) -> f64 {
    // Read latest iq_structure for this (sym, tf).
    // Read latest Wyckoff event row for this (sym, tf) from detections.
    //
    // Score 0..1 based on the canonical alignment matrix in §II.1.
    // Polarity-aware: dip looks for bullish-reversal patterns,
    // top for bearish-reversal patterns.
    //
    // Examples:
    //   Dip + W2 + Spring (within 8 bars)        → 1.0
    //   Dip + C-completed + SOW (within 8 bars)  → 0.9 (reversal imminent)
    //   Dip + W3 + SOS (within 8 bars)           → 0.8 (continuation)
    //   Dip + W2 + ST (low conviction)           → 0.4
    //   Top + W5 + BC (within 8 bars)            → 1.0
    //   Top + B + UTAD                           → 0.95
    //   Top + W3 + SOS (no top here)             → 0.0 (bullish, not top)
    //   No Wyckoff data within horizon           → 0.0 (no signal, not penalty)
}
```

Weight in composite: **0.15** (carved from `multi_tf_confluence`'s
0.10 + reduce one stub by 0.05 to keep total = 1.0). Or extend the
formula to 9 components.

### IV.3 Setup pipeline gate (Layer 2)

In `iq_d_candidate_loop.rs`:

```rust
// After the major-dip-score gate, add Wyckoff alignment check.
let wyckoff_required = load_bool_config("iq_d.require_wyckoff_alignment", true);
if wyckoff_required {
    let alignment = check_wyckoff_alignment(pool, &p, tier).await;
    if alignment == AlignmentVerdict::Conflict {
        // hard skip — Wyckoff says opposite of what Elliott says
        continue;
    }
    // Acceptable: Confirm OR Neutral (no Wyckoff data, fall through).
}
```

`check_wyckoff_alignment` returns:
- `Confirm` — Wyckoff event aligns with Elliott wave intent
- `Neutral` — no Wyckoff data, or unrelated event (no penalty)
- `Conflict` — Wyckoff actively contradicts (e.g., W3 entry tier but
  most recent event is BC = blowoff top)

### IV.4 Entry trigger amplifier (Layer 3)

```sql
-- Setup raw_meta carries wyckoff_event + alignment_verdict so
-- the chart shows "Long IQ-T · Spring + W2 · alignment=confirm
-- · score 0.78"
{
  "iq_structure_id": "...",
  "entry_tier": "W2",
  "wyckoff_event": "spring_bull",
  "wyckoff_phase": "C",
  "alignment_verdict": "confirm",
  "alignment_boost": 1.2
}
```

GUI Analiz page row gets a new **"WYCK"** column showing the latest
Wyckoff phase + dominant event. When the Wyckoff phase strongly
aligns with the IQ wave, the row's Major Dip / Top verdict pill
intensifies (alignment-boosted).

---

## V — Implementation roadmap (this PR + follow-ups)

### V.1 This PR (FAZ 25.4.A)

1. **`qtss-worker::major_dip_candidate_loop`**: add
   `score_wyckoff_alignment` function + register as 9th component.
   Weight 0.15. Reads `detections WHERE pattern_family='wyckoff'`
   for the (sym, tf) within last 50 bars; matches the canonical
   alignment matrix.
2. **Migration 0265**: add `system_config.major_dip.weights.
   wyckoff_alignment = 0.15`; rebalance other weights so sum = 1.0.
3. **`v2_analiz`**: add `wyckoff_phase` (latest A-E phase from
   raw_meta) + `wyckoff_last_event` (subkind + age) per TF snapshot.
4. **`pages/Analiz.tsx`**: render Wyckoff phase pill next to IQ
   wave; tint composite score green/red when alignment is
   confirm/conflict.

### V.2 Follow-up PRs (FAZ 25.4.B — D)

- **B**: hard gate in iq_d / iq_t loops (refuse setup when
  alignment = Conflict).
- **C**: alignment_boost on composite score (multiplier when
  alignment = Confirm).
- **D**: Confluence engine integration — single confluence score
  combining Elliott (motive/abc) + Wyckoff (phase/event) + Harmonic
  + SMC.

---

## VI — References

1. Frost & Prechter — *Elliott Wave Principle* (10th ed) §3 (corrective
   complexity, alternation, equality).
2. David Weis — *Trades About to Happen* (2013) — Weis Wave + Wyckoff
   phase identification.
3. Hank Pruden — *The Three Skills of Top Trading* (2007) — Wyckoff +
   psychology + wave principle synthesis.
4. Bruno DiGiorgi — *Wyckoff Method: Practical Applications in
   Modern Markets* (2019) — phase mapping to current FX/crypto.
5. Richard Wyckoff archives (1908-1934 newsletters, available at
   `wyckoffanalytics.com/archive`).
6. QTSS-internal: `crates/qtss-wyckoff/src/lib.rs`, `crates/qtss-engine/src/writers/wyckoff.rs`,
   `docs/FAZ_10_WYCKOFF_FULL.md` (existing implementation spec).
