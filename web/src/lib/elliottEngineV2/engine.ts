import type {
  CorrectiveCountV2,
  ElliottEngineInputV2,
  ElliottEngineOutputV2,
  ImpulseCountV2,
  OhlcV2,
  Timeframe,
  TimeframeStateV2,
} from "./types";
import { DEFAULT_ELLIOTT_PATTERN_MENU, type ElliottPatternMenuToggles } from "../elliottPatternMenuCatalog";
import { buildZigzagPivotsV2 } from "./zigzag";
import { detectBestImpulseV2, detectHistoricalImpulsesV2 } from "./impulse";
import { detectImpulseCorrectionsV2 } from "./corrective";

function mergePatternToggles(t?: ElliottPatternMenuToggles): ElliottPatternMenuToggles {
  return { ...DEFAULT_ELLIOTT_PATTERN_MENU, ...t };
}

/** 15m mikro itkide dalga 1–5 arası en az bu kadar mum aralığı; aksi halde etiketler üst üste biner. */
const MIN_15M_IMPULSE_P1_P5_BAR_SPAN = 12;

/** Boğa: `w3_not_below_w1_end` — ayı: `w3_not_above_w1_end`. Her itkıda yalnızca biri üretilir; ikisi de set’te olduğu için `decideTimeframeState` hangi yönde fail olursa `invalid` üretir. */
const HARD_IMPULSE_CHECK_IDS = new Set([
  "structure",
  "w2_not_beyond_w1_start",
  "w2_not_longer_than_w1",
  "w3_not_shortest_135",
  "w3_not_below_w1_end",
  "w3_not_above_w1_end",
  "w4_not_longer_than_w3",
  "w5_not_longest_135",
  "ed_r4_w3_area_gt_w2",
  "ld_r3_w5_ge_1382_w4",
]);

function hasPassedCheck(c: CorrectiveCountV2, id: string): boolean {
  return c.checks.some((x) => x.id === id && x.passed);
}

function correctiveIsConfirmed(c: CorrectiveCountV2): boolean {
  if (c.pattern === "zigzag") {
    return hasPassedCheck(c, "abc_order") && hasPassedCheck(c, "zz_r1") && hasPassedCheck(c, "zz_r5") && hasPassedCheck(c, "zz_r6");
  }
  if (c.pattern === "flat") {
    return hasPassedCheck(c, "abc_order") && hasPassedCheck(c, "flat_r4") && hasPassedCheck(c, "flat_g7");
  }
  if (c.pattern === "triangle") {
    const triCore =
      hasPassedCheck(c, "tri_r5") &&
      hasPassedCheck(c, "tri_r2_channel") &&
      hasPassedCheck(c, "tri_r3_apex_after_e") &&
      hasPassedCheck(c, "tri_r4_not_parallel");
    const contract =
      hasPassedCheck(c, "triangle_converging") && hasPassedCheck(c, "triangle_envelope_contract");
    const expand =
      hasPassedCheck(c, "triangle_expanding") && hasPassedCheck(c, "tri_r7_expanding_shortest_ab");
    return triCore && (contract || expand);
  }
  if (c.pattern === "combination") {
    return hasPassedCheck(c, "comb_confirmed") || hasPassedCheck(c, "wxyxz_confirmed");
  }
  return false;
}

export function decideTimeframeState(state: Omit<TimeframeStateV2, "decision">): TimeframeStateV2["decision"] {
  if (!state.impulse) return "invalid";
  const hardFails = state.impulse.checks.some((c) => !c.passed && HARD_IMPULSE_CHECK_IDS.has(c.id));
  // Standard impulse enforces non-overlap. Diagonal variant intentionally allows overlap.
  const standardW4Fail =
    (state.impulse.variant ?? "standard") === "standard" &&
    state.impulse.checks.some((c) => c.id === "w4_no_overlap_w1" && !c.passed);
  if (hardFails) return "invalid";
  if (standardW4Fail) return "invalid";

  /** Dalga 2 ve 4 ikisi de üretildiyse tez beklentisi: her düzeltme kurallara uyar; tek yönlü onay yeterli değil. */
  const internals = [state.wave2, state.wave4].filter((x): x is CorrectiveCountV2 => !!x);
  if (!internals.length) return "candidate";
  return internals.every(correctiveIsConfirmed) ? "confirmed" : "candidate";
}

function microImpulseTooCompressed(imp: ImpulseCountV2): boolean {
  const [, p1, , , , p5] = imp.pivots;
  return p5.index - p1.index < MIN_15M_IMPULSE_P1_P5_BAR_SPAN;
}

function runForTf(tf: Timeframe, rows: OhlcV2[], input: ElliottEngineInputV2): TimeframeStateV2 {
  const pivots = buildZigzagPivotsV2(rows, input.zigzag);
  const menu = mergePatternToggles(input.patternTogglesByTf?.[tf] ?? input.patternToggles);
  let impulse = detectBestImpulseV2(pivots, input.maxWindows ?? 80, {
    allowStandard: menu.motive_impulse,
    allowDiagonal: menu.motive_diagonal,
  });
  let historicalImpulses = detectHistoricalImpulsesV2(pivots, input.maxWindows ?? 240, 16, {
    allowStandard: menu.motive_impulse,
    allowDiagonal: menu.motive_diagonal,
  });
  if (tf === "15m" && impulse && microImpulseTooCompressed(impulse)) {
    impulse = null;
  }
  if (tf === "15m" && historicalImpulses.length) {
    historicalImpulses = historicalImpulses.filter((x) => !microImpulseTooCompressed(x));
  }
  const corr = impulse
    ? detectImpulseCorrectionsV2(pivots, impulse, menu)
    : { wave2: null, wave4: null, postImpulseAbc: null };
  const core = {
    timeframe: tf,
    pivots,
    impulse,
    historicalImpulses,
    wave2: corr.wave2,
    wave4: corr.wave4,
    postImpulseAbc: corr.postImpulseAbc,
  };
  return { ...core, decision: decideTimeframeState(core) };
}

export function runElliottEngineV2(input: ElliottEngineInputV2): ElliottEngineOutputV2 {
  const states: ElliottEngineOutputV2["states"] = {};

  const ordered: Timeframe[] = ["4h", "1h", "15m"];
  for (const tf of ordered) {
    const rows = input.byTimeframe[tf];
    if (!rows?.length) continue;
    states[tf] = runForTf(tf, rows, input);
  }

  return {
    states,
    hierarchy: {
      macro: states["4h"] ?? null,
      intermediate: states["1h"] ?? null,
      micro: states["15m"] ?? null,
    },
    ohlcByTf: input.byTimeframe,
    zigzagParams: input.zigzag,
  };
}

