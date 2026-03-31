/**
 * Consolidated Elliott Wave reference: mandatory rules, guidelines, and common Fibonacci relationships.
 * Sources commonly cited: Frost & Prechter (Elliott Wave Principle), R.N. Elliott, professional summaries (EWI, etc.).
 * Ratios are guidelines unless stated as hard rules; statistics vary by market and sample.
 */

export const ELLIOTT_WAVE_REFERENCE_INTRO =
  "Every valid count must satisfy a small set of inviolable rules and a larger body of guidelines and Fibonacci relationships that narrow the probable path. The framework is fractal: bull-market impulse rules mirror in bear markets (reversed direction).";

/** The three rules that cannot be broken — any violation invalidates a standard impulse count. */
export const ELLIOTT_IMPULSE_INVIOLABLE_RULES: readonly { id: string; text: string }[] = [
  {
    id: "w2_lt_100pct_w1",
    text: "Wave 2 cannot retrace more than 100% of Wave 1. In an uptrend, Wave 2’s low must stay above Wave 1’s origin.",
  },
  {
    id: "w3_not_shortest_135",
    text: "Wave 3 cannot be the shortest of Waves 1, 3, and 5 (typically measured in price extent). Wave 3 is often the longest, but the rule only forbids it being the shortest.",
  },
  {
    id: "w4_no_overlap_w1",
    text: "Wave 4 cannot enter Wave 1’s price territory. In an uptrend, Wave 4’s low cannot drop below Wave 1’s high. Cash markets: strict; futures may show rare intraday exceptions under extreme conditions.",
  },
];

/** Additional mandatory or near-mandatory structural constraints (standard textbook reading). */
export const ELLIOTT_IMPULSE_ADDITIONAL_CONSTRAINTS: readonly string[] = [
  "Wave 3 must travel beyond Wave 1’s endpoint.",
  "Waves 1, 3, and 5 must be motive structures (subdivide into five waves). Wave 3 must be an impulse — never a diagonal.",
  "Waves 2 and 4 must be corrective patterns. Wave 2 cannot be a triangle standing alone.",
  "Truncation (failed fifth): Wave 5 fails to exceed Wave 3’s extreme. Valid only if the truncated fifth still has a complete five-wave internal structure; often follows a very strong Wave 3.",
];

export type ElliottFibonacciGuidelineRow = {
  wave: string;
  measuredAgainst: string;
  commonRatios: string;
  notes?: string;
};

/** Common Fibonacci targets and statistical clusters (guidelines, not hard rules). */
export const ELLIOTT_IMPULSE_FIBONACCI_GUIDELINES: readonly ElliottFibonacciGuidelineRow[] = [
  {
    wave: "Wave 2",
    measuredAgainst: "Wave 1",
    commonRatios: "50%, 61.8%, 76.4%, 78.6%",
    notes: "Many Wave 2s retrace between 50–62% of Wave 1; sharp (zigzag) Wave 2s often cluster near 61.8%.",
  },
  {
    wave: "Wave 3",
    measuredAgainst: "Wave 1",
    commonRatios: "161.8%, 200%, 261.8%, 423.6%",
    notes: "Extension common; a small fraction of Wave 3s are shorter than Wave 1 in practice.",
  },
  {
    wave: "Wave 4",
    measuredAgainst: "Wave 3",
    commonRatios: "23.6%, 38.2%, 50%",
    notes: "Many retrace 30–50% of Wave 3; retracements beyond 50% often invite a recount.",
  },
  {
    wave: "Wave 5 (normal)",
    measuredAgainst: "Wave 1",
    commonRatios: "100% (equality), 61.8%, 161.8%",
    notes: "When Wave 3 extends, Waves 1 and 5 often tend toward equality; 0.618 between the two non-extended legs is a common next case.",
  },
  {
    wave: "Wave 5 (extended)",
    measuredAgainst: "Net of Waves 1–3",
    commonRatios: "61.8%, 100%, 161.8%",
    notes: "Projected from Wave 4’s endpoint; common in commodities.",
  },
];

export const ELLIOTT_IMPULSE_GUIDELINE_BULLETS: readonly string[] = [
  "Alternation (W2 vs W4): sharp vs sideways, simple vs complex; often cited around ~61.8% of the time in long samples.",
  "Extension: typically one of Waves 1, 3, or 5. Equities: Wave 3 extends most often; commodities: Wave 5 often. Extensions can nest.",
  "Channeling: after W3, connect W1–W3 and parallel through W2 for W4 boundary; after W4, connect W2–W4 and parallel through W3 for W5. Throw-over: W5 briefly pierces the channel line.",
  "When Wave 3 extends (>1.618× Wave 1), Wave 5 often equals Wave 1 in price and time.",
  "Wave 4 often retraces into the territory of the previous Wave iv (the fourth wave of one lesser degree within Wave 3).",
];

export const ELLIOTT_LEADING_DIAGONAL_SUMMARY: readonly string[] = [
  "Placement: Wave 1 of an impulse or Wave A of a zigzag — start of a larger pattern.",
  "Overlap: Wave 4 can (and usually does) overlap Wave 1’s territory. Both boundary trendlines slope in the same direction (wedge).",
  "Contracting (common): Wave 1 longest actionary wave, Wave 5 shortest. Expanding: reversed.",
  "Subdivision: classically 5-3-5-3-5; many practitioners also treat 3-3-3-3-3 (all zigzags) as common in practice.",
  "Rules (leading diagonal variant): W2 cannot exceed W1 origin; W3 must exceed W1 end; W3 cannot be shortest; W4 must not move beyond W2’s end. Wave 5 of a leading diagonal is often stated as not truncated — must break beyond Wave 3.",
  "Fibonacci: diagonals often show deeper retracements than standard impulses (e.g. 66–81% of the preceding wave for W2/W4 vs 38–62% in many impulses).",
];

export const ELLIOTT_ENDING_DIAGONAL_SUMMARY: readonly string[] = [
  "Placement: Wave 5 of an impulse or Wave C of a correction — exhaustion after a strong trend.",
  "Subdivision: 3-3-3-3-3 — all five waves are zigzags (simple, double, or triple). Wave 4 almost always overlaps Wave 1.",
  "Contracting ending diagonal: Wave 1 longest, Wave 5 shortest; successive actionary waves often relate by ~0.618 of the previous. Expanding: rare.",
  "Wave 5 may throw over the 1–3 trendline on volume, then reverse sharply; reversal often retraces the full diagonal range or more.",
];

export const ELLIOTT_ZIGZAG_SUMMARY: readonly string[] = [
  "Structure: 5-3-5. Wave A: five-wave (impulse or leading diagonal). Wave B: any corrective. Wave C: five-wave (impulse or ending diagonal).",
  "Wave B cannot retrace beyond Wave A’s origin.",
  "Wave B often retraces 38.2–61.8% of A (form-dependent). Wave C often targets equality with A (100%), then 161.8% or 61.8% of A; C usually extends beyond A’s end.",
  "Double (W–X–Y) and triple (W–X–Y–X–Z) zigzags when one zigzag under-retraces. X connectors often cluster at 38.2%, 50%, or 61.8% of the prior leg.",
];

export const ELLIOTT_FLAT_SUMMARY: readonly string[] = [
  "Structure: 3-3-5. Waves A and B are three-wave correctives; Wave C is five-wave.",
  "Distinguishing guideline: flat B often retraces at least ~90% of A (regular flat ~90–105%; expanded often >100% of A, e.g. 123.6% or 138.2%; C may extend to ~161.8% of A).",
  "Running flat: B >100% of A but C falls short of A’s end — rare.",
];

export const ELLIOTT_TRIANGLE_SUMMARY: readonly string[] = [
  "Structure: 3-3-3-3-3. Five overlapping corrective waves A–E, each typically three waves (often zigzags).",
  "Placement: Wave 4 of an impulse, Wave B of a zigzag or flat, Wave X in certain combinations, or final pattern in a combination. Not as standalone Wave 2.",
  "Contracting: B < A, C < B, D < C, E < D; trendlines converge. Common Fibonacci cascade: each leg often ~0.618 of the prior; B often ~61.8–78.6% of A; B often retraces at least ~50% of A.",
  "Expanding: B > A, C > B, D > C; rare. Successive waves may relate by ~105–161.8% of the prior.",
  "Post-triangle thrust often relates to the widest part of the triangle; apex timing often coincides with a significant turn.",
];

export const ELLIOTT_COMBINATION_SUMMARY: readonly string[] = [
  "Lateral combinations: W–X–Y and W–X–Y–X–Z built from simpler correctives linked by X waves.",
  "Common constraints in professional literature: at most one simple zigzag in a combination; at most one triangle, and it is usually the final segment; components often alternate in form.",
  "Maximum three corrective patterns (triple three is the limit). X-wave depth rules differ for sideways vs zigzag-family combinations.",
];

export type ElliottPositionSubstructureRow = {
  position: string;
  requiredStructure: string;
  allowedPatterns: string;
};

/** What each position must contain at the next lower degree (textbook structural expectations). */
export const ELLIOTT_POSITION_SUBSTRUCTURE: readonly ElliottPositionSubstructureRow[] = [
  { position: "Wave 1 (impulse)", requiredStructure: "5-wave motive", allowedPatterns: "Impulse (5-3-5-3-5) or leading diagonal" },
  { position: "Wave 2 (impulse)", requiredStructure: "3-wave corrective", allowedPatterns: "Zigzag, flat, combination — not a triangle alone" },
  { position: "Wave 3 (impulse)", requiredStructure: "5-wave motive", allowedPatterns: "Impulse only — diagonal excluded (strict reading)" },
  { position: "Wave 4 (impulse)", requiredStructure: "3-wave corrective", allowedPatterns: "Zigzag, flat, triangle, or combination" },
  { position: "Wave 5 (impulse)", requiredStructure: "5-wave motive", allowedPatterns: "Impulse (5-3-5-3-5) or ending diagonal (3-3-3-3-3)" },
  { position: "Wave A (zigzag)", requiredStructure: "5-wave motive", allowedPatterns: "Impulse or leading diagonal" },
  { position: "Wave B (zigzag)", requiredStructure: "3-wave corrective", allowedPatterns: "Any corrective" },
  { position: "Wave C (zigzag)", requiredStructure: "5-wave motive", allowedPatterns: "Impulse or ending diagonal" },
  { position: "Wave A (flat)", requiredStructure: "3-wave corrective", allowedPatterns: "Any corrective except triangle" },
  { position: "Wave B (flat)", requiredStructure: "3-wave corrective", allowedPatterns: "Any corrective except triangle" },
  { position: "Wave C (flat)", requiredStructure: "5-wave motive", allowedPatterns: "Impulse or ending diagonal" },
  { position: "Waves A–E (triangle)", requiredStructure: "3-wave corrective", allowedPatterns: "Zigzag (common), flat, or smaller triangle" },
];

/** Classical nine degrees (largest to smallest); time labels are typical, not definitional. */
export const ELLIOTT_WAVE_DEGREES_CLASSICAL: readonly string[] = [
  "Grand Supercycle",
  "Supercycle",
  "Cycle",
  "Primary",
  "Intermediate",
  "Minor",
  "Minute",
  "Minuette",
  "Subminuette",
];

export const ELLIOTT_FIBONACCI_CYCLE_NOTE =
  "Fractal cycle counts: one complete cycle is 2 waves at the highest level; first subdivision 8 (5+3); second 34 (21+13); third 144 (89+55) — all Fibonacci numbers in the standard progression.";

export const ELLIOTT_BEAR_MIRROR_NOTE =
  "Bear markets mirror bull markets: the same rules apply in reverse. Motive waves need not point up; corrective waves need not point down — mode is determined by relative direction within the trend.";
