import type {
  CorrectiveCountV2,
  DiagonalRoleSourceV2,
  DiagonalRoleV2,
  ElliottEngineInputV2,
  ElliottEngineOutputV2,
  ElliottRuleCheckV2,
  ImpulseCountV2,
  ImpulseDirectionV2,
  OhlcV2,
  Timeframe,
  TimeframeStateV2,
  ZigzagParams,
  ZigzagPivot,
} from "./types";
import {
  DEFAULT_ELLIOTT_PATTERN_MENU,
  patternMenuAllowDiagonal,
  type ElliottPatternMenuToggles,
} from "../elliottPatternMenuCatalog";
import { pickMtfDiagonalRoleFor1hTf, pickMtfDiagonalRoleForMicroTf } from "./diagonalMtf";
import { buildZigzagPivotsV2 } from "./zigzag";
import { inferDiagonalRoleFromChart } from "./inferDiagonalRole";
import { detectBestImpulseV2, detectHistoricalImpulsesV2, detectNestedImpulseInLeg } from "./impulse";
import { detectImpulseCorrectionsV2, detectNestedAbcCorrectiveInLeg, detectNestedCorrectiveInLeg } from "./corrective";

function mergePatternToggles(t?: ElliottPatternMenuToggles): ElliottPatternMenuToggles {
  return { ...DEFAULT_ELLIOTT_PATTERN_MENU, ...t };
}

function impulseDetectOptsFromMenu(menu: ElliottPatternMenuToggles) {
  const allowDiag = patternMenuAllowDiagonal(menu);
  return {
    allowStandard: menu.motive_impulse,
    allowDiagonal: allowDiag,
    allowDiagonalLeading: menu.motive_diagonal_leading,
    allowDiagonalEnding: menu.motive_diagonal_ending,
  };
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
]);

function hasPassedCheck(c: CorrectiveCountV2, id: string): boolean {
  return c.checks.some((x) => x.id === id && x.passed);
}

function correctiveIsConfirmed(c: CorrectiveCountV2): boolean {
  if (c.pattern === "zigzag") {
    return (
      hasPassedCheck(c, "abc_order") &&
      hasPassedCheck(c, "zz_r1") &&
      hasPassedCheck(c, "zz_r5") &&
      hasPassedCheck(c, "zz_r6") &&
      hasPassedCheck(c, "zz_a_motive5") &&
      hasPassedCheck(c, "zz_c_motive5")
    );
  }
  if (c.pattern === "flat") {
    return (
      hasPassedCheck(c, "abc_order") &&
      hasPassedCheck(c, "flat_r4") &&
      hasPassedCheck(c, "flat_g7") &&
      hasPassedCheck(c, "flat_ba_label_floor") &&
      hasPassedCheck(c, "flat_a_corrective3") &&
      hasPassedCheck(c, "flat_b_corrective3") &&
      hasPassedCheck(c, "flat_c_motive5")
    );
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
    const legs =
      hasPassedCheck(c, "tri_a_corrective3") &&
      hasPassedCheck(c, "tri_b_corrective3") &&
      hasPassedCheck(c, "tri_c_corrective3") &&
      hasPassedCheck(c, "tri_d_corrective3") &&
      hasPassedCheck(c, "tri_e_corrective3");
    return triCore && (contract || expand) && legs;
  }
  if (c.pattern === "combination") {
    return hasPassedCheck(c, "comb_confirmed") || hasPassedCheck(c, "wxyxz_confirmed");
  }
  return false;
}

/** Tez §2.5.3.4 ld_r3 applies to leading diagonals; ending/unknown should not be scored against it. */
function mapLdR3ByDiagonalRole(checks: ElliottRuleCheckV2[], role: DiagonalRoleV2): ElliottRuleCheckV2[] {
  if (role === "leading") return checks;
  return checks.map((c) =>
    c.id === "ld_r3_w5_ge_1382_w4"
      ? {
          ...c,
          passed: true,
          detail:
            "ld_r3 (leading diagonal: |5| ≥ 1.382 × |4|) not applied; chart role is ending/unknown.",
        }
      : c,
  );
}

type DiagonalProofContext = {
  pivots: ZigzagPivot[];
  rows: OhlcV2[];
  input: ElliottEngineInputV2;
  menu: ElliottPatternMenuToggles;
};

type DiagonalProofOverride = {
  role: DiagonalRoleV2;
  source: DiagonalRoleSourceV2;
  mtfDetail?: string;
};

/**
 * Append ld_r3 mapping, role hint, and nested diagonal leg checks (leading vs ending/unknown).
 */
function buildDiagonalSubwaveProof(
  imp: ImpulseCountV2 | null,
  ctx: DiagonalProofContext,
  override?: DiagonalProofOverride,
): ImpulseCountV2 | null {
  if (!imp || (imp.variant ?? "standard") !== "diagonal") return imp;
  const role = override?.role ?? imp.diagonalRole ?? inferDiagonalRoleFromChart(imp, ctx.pivots);
  const source: DiagonalRoleSourceV2 = override?.source ?? "chart_window";
  const ldMapped = mapLdR3ByDiagonalRole(imp.checks, role);
  const scoredBase: ImpulseCountV2 = {
    ...imp,
    checks: ldMapped,
    score: ldMapped.filter((c) => c.passed).length,
    diagonalRole: role,
    diagonalRoleSource: source,
  };
  const [p0, p1, p2, p3, p4, p5] = scoredBase.pivots;
  const isBull = scoredBase.direction === "bull";
  const dir2 = isBull ? ("down" as const) : ("up" as const);
  const nestedMotiveNoDiag = {
    allowStandard: ctx.menu.motive_impulse,
    allowDiagonal: false,
  };
  const hintDetail =
    source === "mtf_parent" && override?.mtfDetail
      ? `heuristic=${role} (${override.mtfDetail})`
      : `heuristic=${role} (zigzag pivot window: early start vs late end)`;
  const withRole: ImpulseCountV2 = {
    ...scoredBase,
    checks: [
      ...scoredBase.checks,
      {
        id: "diagonal_role_hint",
        passed: true,
        detail: hintDetail,
      },
    ],
  };

  if (role === "leading") {
    const w1 = detectNestedImpulseInLeg(ctx.pivots, p0, p1, nestedMotiveNoDiag, ctx.rows, ctx.input.zigzag);
    const w2 = detectNestedCorrectiveInLeg(ctx.pivots, p1, p2, dir2, "wave2", ctx.menu, ctx.rows, ctx.input.zigzag);
    const w3 = detectNestedImpulseInLeg(ctx.pivots, p2, p3, nestedMotiveNoDiag, ctx.rows, ctx.input.zigzag);
    const w4 = detectNestedCorrectiveInLeg(ctx.pivots, p3, p4, dir2, "wave4", ctx.menu, ctx.rows, ctx.input.zigzag);
    const w5 = detectNestedImpulseInLeg(ctx.pivots, p4, p5, nestedMotiveNoDiag, ctx.rows, ctx.input.zigzag);
    const extra = [
      {
        id: "diag_ld_1_motive5",
        passed: !!w1,
        detail: w1 ? "Leg 1 nested motive evidence" : "Leg 1 motive proof missing",
      },
      {
        id: "diag_ld_2_corr3",
        passed: !!w2,
        detail: w2 ? "Leg 2 nested corrective evidence" : "Leg 2 corrective proof missing",
      },
      {
        id: "diag_ld_3_motive5",
        passed: !!w3,
        detail: w3 ? "Leg 3 nested motive evidence" : "Leg 3 motive proof missing",
      },
      {
        id: "diag_ld_4_corr3",
        passed: !!w4,
        detail: w4 ? "Leg 4 nested corrective evidence" : "Leg 4 corrective proof missing",
      },
      {
        id: "diag_ld_5_motive5",
        passed: !!w5,
        detail: w5 ? "Leg 5 nested motive evidence" : "Leg 5 motive proof missing",
      },
    ];
    return { ...withRole, checks: [...withRole.checks, ...extra] };
  }

  const dirs: readonly ("up" | "down")[] =
    scoredBase.direction === "bull"
      ? ["up", "down", "up", "down", "up"]
      : ["down", "up", "down", "up", "down"];
  const legs: readonly [ZigzagPivot, ZigzagPivot][] = [
    [p0, p1],
    [p1, p2],
    [p2, p3],
    [p3, p4],
    [p4, p5],
  ];
  const ids = [
    "diag_1_corrective3",
    "diag_2_corrective3",
    "diag_3_corrective3",
    "diag_4_corrective3",
    "diag_5_corrective3",
  ] as const;
  const extra = ids.map((id, i) => {
    const [a, b] = legs[i]!;
    const hit = detectNestedAbcCorrectiveInLeg(a, b, dirs[i]!, ctx.rows, ctx.input.zigzag);
    return {
      id,
      passed: !!hit,
      detail: hit ? "Diagonal leg has nested ABC corrective evidence" : "Diagonal leg corrective-3 proof missing",
    };
  });
  return { ...withRole, checks: [...withRole.checks, ...extra] };
}

function refineDiagonalStateWithMtf(
  tf: Timeframe,
  state: TimeframeStateV2,
  parents: { s1h: TimeframeStateV2 | null | undefined; s4h: TimeframeStateV2 | null | undefined },
  input: ElliottEngineInputV2,
): TimeframeStateV2 {
  const orig = state.impulse;
  if (!orig || (orig.variant ?? "standard") !== "diagonal") return state;

  const menu = mergePatternToggles(input.patternTogglesByTf?.[tf] ?? input.patternToggles);
  const rows = input.byTimeframe[tf];
  if (!rows?.length) return state;
  const depth = input.zigzagDepthByTimeframe?.[tf] ?? input.zigzag.depth;
  const zigzag = { ...input.zigzag, depth };
  const fullInput: ElliottEngineInputV2 = { ...input, zigzag };
  const pivots = state.pivots;

  const fresh = detectBestImpulseV2(pivots, input.maxWindows ?? 80, impulseDetectOptsFromMenu(menu));
  if (tf === "15m" && fresh && microImpulseTooCompressed(fresh)) {
    return state;
  }
  if (!fresh || fresh.variant !== "diagonal") return state;
  if (fresh.pivots[0].index !== orig.pivots[0].index || fresh.pivots[5].index !== orig.pivots[5].index) {
    return state;
  }

  const mtf =
    tf === "15m"
      ? pickMtfDiagonalRoleForMicroTf(fresh, parents.s1h, parents.s4h)
      : tf === "1h"
        ? pickMtfDiagonalRoleFor1hTf(fresh, parents.s4h)
        : null;
  if (!mtf) return state;

  const newImp = buildDiagonalSubwaveProof(fresh, { pivots, rows, input: fullInput, menu }, {
    role: mtf.role,
    source: "mtf_parent",
    mtfDetail: mtf.detail,
  });
  if (!newImp) return state;

  const core: Omit<TimeframeStateV2, "decision"> = {
    ...state,
    impulse: newImp,
  };
  return { ...core, decision: decideTimeframeState(core) };
}

function applyMtfDiagonalRefinement(
  states: Partial<Record<Timeframe, TimeframeStateV2>>,
  input: ElliottEngineInputV2,
): void {
  const s4h = states["4h"];
  const s1h = states["1h"];
  const s15 = states["15m"];
  if (s15) {
    states["15m"] = refineDiagonalStateWithMtf("15m", s15, { s1h, s4h }, input);
  }
  if (s1h) {
    states["1h"] = refineDiagonalStateWithMtf("1h", s1h, { s1h: undefined, s4h }, input);
  }
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

  /**
   * Truncation guard: if Wave 5 did not exceed Wave 3 (failed fifth), do not allow `confirmed`
   * unless wave-5 internal motive evidence exists at a lower degree.
   *
   * This keeps the wide-net impulse detector intact, but aligns confidence with strict theory:
   * truncation is only meaningful when W5 still subdivides as a valid five.
   */
  const isFailedFifth = state.impulse.checks.some((c) => c.id === "extension_w5_vs_w3" && !c.passed);
  if (isFailedFifth) {
    const w5 = state.wave5NestedImpulse;
    if (!w5) return "candidate";
    /** Truncation proof: inner five must be a standard impulse, not a diagonal. */
    if (w5.variant === "diagonal") return "candidate";
  }

  /**
   * Diagonal subwave composition guard:
   * - `leading`: nested motive on 1/3/5 + nested corrective on 2/4 (5-3-5-3-5 heuristic).
   * - `ending` / `unknown`: nested ABC corrective on all five legs (3-3-3-3-3 heuristic).
   */
  const isDiagonal = (state.impulse.variant ?? "standard") === "diagonal";
  if (isDiagonal) {
    const role = state.impulse.diagonalRole ?? "unknown";
    const required =
      role === "leading"
        ? ([
            "diag_ld_1_motive5",
            "diag_ld_2_corr3",
            "diag_ld_3_motive5",
            "diag_ld_4_corr3",
            "diag_ld_5_motive5",
          ] as const)
        : ([
            "diag_1_corrective3",
            "diag_2_corrective3",
            "diag_3_corrective3",
            "diag_4_corrective3",
            "diag_5_corrective3",
          ] as const);
    const ok = required.every((id) => state.impulse!.checks.some((c) => c.id === id && c.passed));
    if (!ok) return "candidate";
  }

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
  const detectOpts = impulseDetectOptsFromMenu(menu);
  let impulse = detectBestImpulseV2(pivots, input.maxWindows ?? 80, detectOpts);
  let historicalImpulses = detectHistoricalImpulsesV2(pivots, input.maxWindows ?? 240, 16, detectOpts);
  if (tf === "15m" && impulse && microImpulseTooCompressed(impulse)) {
    impulse = null;
  }
  if (tf === "15m" && historicalImpulses.length) {
    historicalImpulses = historicalImpulses.filter((x) => !microImpulseTooCompressed(x));
  }
  const corr = impulse
    ? detectImpulseCorrectionsV2(pivots, impulse, menu)
    : { wave2: null, wave4: null, postImpulseAbc: null };

  const zigzagMotiveProofOpts = { allowStandard: true, allowDiagonal: true };
  function addZigzagMotiveProof(c: CorrectiveCountV2 | null): CorrectiveCountV2 | null {
    if (!c || c.pattern !== "zigzag") return c;
    const [start, a, b, end] = c.pivots;
    const aMotive = detectNestedImpulseInLeg(pivots, start, a, zigzagMotiveProofOpts, rows, input.zigzag);
    const cMotive = detectNestedImpulseInLeg(pivots, b, end, zigzagMotiveProofOpts, rows, input.zigzag);
    const extra = [
      {
        id: "zz_a_motive5",
        passed: !!aMotive,
        detail: aMotive ? "A has nested motive evidence" : "A motive proof missing",
      },
      {
        id: "zz_c_motive5",
        passed: !!cMotive,
        detail: cMotive ? "C has nested motive evidence" : "C motive proof missing",
      },
    ];
    return { ...c, checks: [...c.checks, ...extra] };
  }

  function addFlatStructureProof(
    c: CorrectiveCountV2 | null,
    parentContext: "wave2" | "wave4",
    impulseDirection: ImpulseDirectionV2,
  ): CorrectiveCountV2 | null {
    if (!c || c.pattern !== "flat") return c;
    const [start, a, b, end] = c.pivots;
    const dirs =
      impulseDirection === "bull"
        ? { a: "down" as const, b: "up" as const, c: "down" as const }
        : { a: "up" as const, b: "down" as const, c: "up" as const };
    const aCorr = detectNestedCorrectiveInLeg(pivots, start, a, dirs.a, parentContext, menu, rows, input.zigzag);
    const bCorr = detectNestedCorrectiveInLeg(pivots, a, b, dirs.b, parentContext, menu, rows, input.zigzag);
    const cMotive = detectNestedImpulseInLeg(pivots, b, end, zigzagMotiveProofOpts, rows, input.zigzag);
    const extra = [
      {
        id: "flat_a_corrective3",
        passed: !!aCorr,
        detail: aCorr ? "A has nested corrective evidence" : "A corrective proof missing",
      },
      {
        id: "flat_b_corrective3",
        passed: !!bCorr,
        detail: bCorr ? "B has nested corrective evidence" : "B corrective proof missing",
      },
      {
        id: "flat_c_motive5",
        passed: !!cMotive,
        detail: cMotive ? "C has nested motive evidence" : "C motive proof missing",
      },
    ];
    return { ...c, checks: [...c.checks, ...extra] };
  }

  function addTriangleLegProof(
    c: CorrectiveCountV2 | null,
    parentContext: "wave2" | "wave4",
    impulseDirection: ImpulseDirectionV2,
  ): CorrectiveCountV2 | null {
    if (!c || c.pattern !== "triangle" || !c.path || c.path.length < 6) return c;
    const path = c.path;
    const dirs: readonly ("up" | "down")[] =
      impulseDirection === "bull"
        ? ["down", "up", "down", "up", "down"]
        : ["up", "down", "up", "down", "up"];
    const legIds = [
      "tri_a_corrective3",
      "tri_b_corrective3",
      "tri_c_corrective3",
      "tri_d_corrective3",
      "tri_e_corrective3",
    ] as const;
    const extra = legIds.map((id, i) => {
      const hit = detectNestedCorrectiveInLeg(pivots, path[i]!, path[i + 1]!, dirs[i]!, parentContext, menu, rows, input.zigzag);
      const label = id.replace("tri_", "").replace("_corrective3", "");
      return {
        id,
        passed: !!hit,
        detail: hit ? `${label} leg: nested corrective evidence` : `${label} leg: corrective-3 proof missing`,
      };
    });
    return { ...c, checks: [...c.checks, ...extra] };
  }

  const nestedImpulseOpts = {
    allowStandard: menu.motive_impulse,
    allowDiagonal: patternMenuAllowDiagonal(menu),
    allowDiagonalLeading: menu.motive_diagonal_leading,
    allowDiagonalEnding: menu.motive_diagonal_ending,
  };
  let wave1NestedImpulse: ImpulseCountV2 | null = null;
  let wave2NestedCorrective: CorrectiveCountV2 | null = null;
  let wave3NestedImpulse: ImpulseCountV2 | null = null;
  let wave4NestedCorrective: CorrectiveCountV2 | null = null;
  let wave5NestedImpulse: ImpulseCountV2 | null = null;
  if (impulse) {
    const [p0, p1, p2, p3, p4, p5] = impulse.pivots;
    const isBull = impulse.direction === "bull";
    const dir2 = isBull ? ("down" as const) : ("up" as const);
    wave1NestedImpulse = detectNestedImpulseInLeg(pivots, p0, p1, nestedImpulseOpts, rows, input.zigzag);
    wave2NestedCorrective = detectNestedCorrectiveInLeg(pivots, p1, p2, dir2, "wave2", menu, rows, input.zigzag);
    wave3NestedImpulse = detectNestedImpulseInLeg(
      pivots,
      p2,
      p3,
      { ...nestedImpulseOpts, allowDiagonal: false },
      rows,
      input.zigzag,
    );
    wave4NestedCorrective = detectNestedCorrectiveInLeg(pivots, p3, p4, dir2, "wave4", menu, rows, input.zigzag);
    // Same menu as wave-1 nested: w4–w1 overlap is invalid for standard but valid for diagonal (§2.5.3.4).
    wave5NestedImpulse = detectNestedImpulseInLeg(pivots, p4, p5, nestedImpulseOpts, rows, input.zigzag);
  }

  const historicalImpulseExtras = historicalImpulses.map((hi) => {
    const raw = detectImpulseCorrectionsV2(pivots, hi, menu);
    const dir = hi.direction;
    return {
      wave2: addTriangleLegProof(
        addFlatStructureProof(addZigzagMotiveProof(raw.wave2), "wave2", dir),
        "wave2",
        dir,
      ),
      wave4: addTriangleLegProof(
        addFlatStructureProof(addZigzagMotiveProof(raw.wave4), "wave4", dir),
        "wave4",
        dir,
      ),
      postImpulseAbc: raw.postImpulseAbc,
    };
  });

  const core = {
    timeframe: tf,
    pivots,
    impulse: buildDiagonalSubwaveProof(impulse, { pivots, rows, input, menu }),
    wave1NestedImpulse,
    wave2NestedCorrective,
    wave3NestedImpulse,
    wave4NestedCorrective,
    wave5NestedImpulse,
    historicalImpulses,
    historicalImpulseExtras,
    wave2: addTriangleLegProof(
      addFlatStructureProof(
        addZigzagMotiveProof(corr.wave2),
        "wave2",
        impulse?.direction ?? "bull",
      ),
      "wave2",
      impulse?.direction ?? "bull",
    ),
    wave4: addTriangleLegProof(
      addFlatStructureProof(
        addZigzagMotiveProof(corr.wave4),
        "wave4",
        impulse?.direction ?? "bull",
      ),
      "wave4",
      impulse?.direction ?? "bull",
    ),
    postImpulseAbc: corr.postImpulseAbc,
  };
  return { ...core, decision: decideTimeframeState(core) };
}

export function runElliottEngineV2(input: ElliottEngineInputV2): ElliottEngineOutputV2 {
  const states: ElliottEngineOutputV2["states"] = {};
  const zigzagParamsByTf: Partial<Record<Timeframe, ZigzagParams>> = {};

  const ordered: Timeframe[] = ["1w", "1d", "4h", "1h", "15m"];
  for (const tf of ordered) {
    const rows = input.byTimeframe[tf];
    if (!rows?.length) continue;
    const depth = input.zigzagDepthByTimeframe?.[tf] ?? input.zigzag.depth;
    const zigzag = { ...input.zigzag, depth };
    zigzagParamsByTf[tf] = zigzag;
    states[tf] = runForTf(tf, rows, { ...input, zigzag });
  }

  applyMtfDiagonalRefinement(states, input);

  return {
    states,
    hierarchy: {
      macro: states["4h"] ?? null,
      intermediate: states["1h"] ?? null,
      micro: states["15m"] ?? null,
    },
    ohlcByTf: input.byTimeframe,
    zigzagParams: input.zigzag,
    zigzagParamsByTf,
  };
}

