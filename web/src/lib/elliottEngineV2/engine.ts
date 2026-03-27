import type {
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

function decide(state: Omit<TimeframeStateV2, "decision">): TimeframeStateV2["decision"] {
  if (!state.impulse) return "invalid";
  const hardFails = state.impulse.checks.some(
    (c) =>
      !c.passed &&
      (c.id === "structure" || c.id === "w2_not_beyond_w1_start" || c.id === "w3_not_shortest_135" || c.id === "w4_no_overlap_w1"),
  );
  if (hardFails) return "invalid";
  const hasInternal = !!state.wave2 || !!state.wave4;
  return hasInternal ? "confirmed" : "candidate";
}

function microImpulseTooCompressed(imp: ImpulseCountV2): boolean {
  const [, p1, , , , p5] = imp.pivots;
  return p5.index - p1.index < MIN_15M_IMPULSE_P1_P5_BAR_SPAN;
}

function runForTf(tf: Timeframe, rows: OhlcV2[], input: ElliottEngineInputV2): TimeframeStateV2 {
  const pivots = buildZigzagPivotsV2(rows, input.zigzag);
  const menu = mergePatternToggles(input.patternToggles);
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
    ? detectImpulseCorrectionsV2(pivots, impulse, input.patternToggles)
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
  return { ...core, decision: decide(core) };
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

