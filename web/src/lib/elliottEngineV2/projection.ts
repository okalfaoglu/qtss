import type { UTCTimestamp } from "lightweight-charts";
import {
  collectAbcCorrectivesWithFixedAB,
  collectAllAbcPostImpulseCandidates,
} from "./corrective";
import { DEFAULT_ELLIOTT_PATTERN_MENU, type ElliottPatternMenuToggles } from "../elliottPatternMenuCatalog";
import { patternMenuAllowsFlatAbc, patternMenuAllowsZigzagAbc } from "./corrective";
import type { PatternLayerOverlay } from "../patternDrawingBatchOverlay";
import type {
  CorrectiveCountV2,
  ElliottEngineOutputV2,
  ImpulseCountV2,
  OhlcV2,
  Timeframe,
  ZigzagPivot,
} from "./types";

/**
 * Post–P5 düzeltmede `path` tüm zigzag uçlarını içerir; W–X–Y (ve ABC) **köşeleri** `pivots` içindedir.
 * Projeksiyon `path[1..3]` ile yapılırsa X yanlış köşe olur ve “+Y’den sonra” çizgi yönü bozulur.
 */
function postImpulseStructuralGuide(post: CorrectiveCountV2 | null): null | {
  donePolyline: ZigzagPivot[];
  a: ZigzagPivot;
  b: ZigzagPivot;
  c: ZigzagPivot | null;
  hasA: boolean;
  hasB: boolean;
  hasC: boolean;
  /** Düzeltmenin bitiş köşesi (C / Y / E); sonraki itkı buradan başlar. */
  patternEnd: ZigzagPivot;
} {
  if (!post) return null;
  const donePolyline = post.path?.length ? post.path : [...post.pivots];
  if (post.pattern === "triangle") {
    const path = post.path?.length ? post.path : post.pivots;
    if (path.length < 3) return null;
    const patternEnd = path[path.length - 1]!;
    return {
      donePolyline,
      a: path[1]!,
      b: path[2]!,
      c: path.length >= 4 ? path[3]! : null,
      hasA: path.length >= 2,
      hasB: path.length >= 3,
      hasC: path.length >= 4,
      patternEnd,
    };
  }
  const pv = post.pivots;
  const hasA = pv.length >= 2;
  const hasB = pv.length >= 3;
  const hasC = pv.length >= 4;
  if (!hasA) return null;
  const a = pv[1]!;
  const b = hasB ? pv[2]! : a;
  const c = hasC ? pv[3]! : null;
  const patternEnd = hasC ? pv[3]! : hasB ? pv[2]! : pv[1]!;
  return { donePolyline, a, b, c, hasA, hasB, hasC, patternEnd };
}

function impulseShownInMenu(imp: ImpulseCountV2, menu?: ElliottPatternMenuToggles): boolean {
  const m = { ...DEFAULT_ELLIOTT_PATTERN_MENU, ...menu };
  const v = imp.variant ?? "standard";
  if (v === "diagonal") {
    const role = imp.diagonalRole ?? "unknown";
    if (role === "leading") return m.motive_diagonal_leading;
    if (role === "ending") return m.motive_diagonal_ending;
    return m.motive_diagonal_leading || m.motive_diagonal_ending;
  }
  return m.motive_impulse;
}

type Pt = { time: UTCTimestamp; value: number };

export type ElliottProjectionV2Options = {
  /**
   * Second formation path: alternate Fib calibration (extended wave-3 style).
   * @default true
   */
  includeAltScenario?: boolean;
  /** Multiple zigzag vs flat (and pivot ABC) hypotheses in distinct colors. */
  multiCorrectiveScenarios?: boolean;
};

function chooseImpulse(out: ElliottEngineOutputV2) {
  return out.hierarchy.micro?.impulse ?? out.hierarchy.intermediate?.impulse ?? out.hierarchy.macro?.impulse ?? null;
}

function impulseForProjectionTf(out: ElliottEngineOutputV2, tf: Timeframe): ImpulseCountV2 | null {
  return out.states[tf]?.impulse ?? null;
}

function postAbcForProjectionTf(out: ElliottEngineOutputV2, tf: Timeframe): CorrectiveCountV2 | null {
  return out.states[tf]?.postImpulseAbc ?? null;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

/** Son ~14 mum için ortalama true range — itkı bacak ortalaması ile karıştırılarak projeksiyon genliği ölçeklenir. */
function inferAtr14(rows: OhlcV2[]): number {
  if (rows.length < 2) return 0;
  const period = 14;
  const start = Math.max(1, rows.length - period);
  let sum = 0;
  let n = 0;
  for (let i = start; i < rows.length; i++) {
    const row = rows[i]!;
    const prev = rows[i - 1]!;
    const h = row.h ?? row.c;
    const l = row.l ?? row.c;
    const tr = Math.max(h - l, Math.abs(h - prev.c), Math.abs(l - prev.c));
    sum += tr;
    n++;
  }
  return n > 0 ? sum / n : 0;
}

/**
 * İtkı 1/3/5 ortalama genliği + güncel ATR: son dönem volatilitesi itkıdan yüksekse adımları büyütür (dar / uçuk projeksiyonu azaltır).
 */
function blendedProjectionPriceBase(imp: ImpulseCountV2, rows: OhlcV2[]): number {
  const p = imp.pivots;
  const len1 = Math.abs(p[1].price - p[0].price);
  const len3 = Math.abs(p[3].price - p[2].price);
  const len5 = Math.abs(p[5].price - p[4].price);
  const legAvg = (len1 + len3 + len5) / 3;
  const atr = inferAtr14(rows);
  if (!Number.isFinite(atr) || atr <= 1e-12) return Math.max(1e-8, legAvg);
  const r = atr / Math.max(legAvg, 1e-12);
  const boost = clamp((r - 1) * 0.35, 0, 0.95);
  return Math.max(1e-8, legAvg * (1 + boost));
}

type FormationPoint = { time: UTCTimestamp; value: number };

function seg(a: FormationPoint, b: FormationPoint, kind: "elliott_projection" | "elliott_projection_alt", style: "solid" | "dotted" | "dashed", lineColor?: string): PatternLayerOverlay {
  return {
    upper: [],
    lower: [],
    zigzag: [a, b],
    zigzagKind: kind,
    zigzagLineColor: lineColor,
    zigzagLineStyle: style,
  };
}

function horizLevel(
  fromT: number,
  toT: number,
  price: number,
  kind: "elliott_projection_target" | "elliott_projection_target_alt",
  lineColor?: string,
): PatternLayerOverlay {
  return {
    upper: [
      { time: fromT as UTCTimestamp, value: price },
      { time: toT as UTCTimestamp, value: price },
    ],
    lower: [],
    zigzag: [],
    zigzagKind: kind,
    zigzagLineColor: lineColor,
    zigzagLineStyle: "dotted",
    zigzagLineWidth: 1,
  };
}

function buildFormationProjection(params: {
  imp: ImpulseCountV2;
  postAbc: CorrectiveCountV2 | null;
  startTime: number;
  startPrice: number;
  tf: Timeframe;
  lineColor?: string;
  alt: boolean;
}): { layers: PatternLayerOverlay[] } {
  const { imp, postAbc, startTime, startPrice, tf, lineColor, alt } = params;
  const kind = alt ? "elliott_projection_alt" : "elliott_projection";
  const targetKind = alt ? "elliott_projection_target_alt" : "elliott_projection_target";
  const layers: PatternLayerOverlay[] = [];
  const p = imp.pivots;
  const p0 = p[0], p1 = p[1], p2 = p[2], p3 = p[3], p4 = p[4], p5 = p[5];
  const isBull = imp.direction === "bull";
  const dir = isBull ? 1 : -1;

  // Helpers
  const impulseSize = Math.abs(p5.price - p0.price);
  const w1Size = Math.abs(p1.price - p0.price);
  const w1Dur = Math.max(60, p1.time - p0.time);
  const w2Dur = Math.max(60, p2.time - p1.time);
  const w3Dur = Math.max(60, p3.time - p2.time);

  const projAVal = isBull ? p5.price - impulseSize * 0.382 : p5.price + impulseSize * 0.382;
  const abSize = Math.abs(projAVal - p5.price);
  const projBVal = isBull ? projAVal + abSize * 0.618 : projAVal - abSize * 0.618;
  const projCVal = isBull ? projBVal - abSize * 1.0 : projBVal + abSize * 1.0;

  const minSegDur = Math.max(120, Math.round(w1Dur * 0.35));
  const durationA = Math.max(minSegDur, Math.round(w1Dur * 0.618));
  const durationB = Math.max(minSegDur, Math.round(w2Dur * 1.0));
  const durationC = Math.max(minSegDur, Math.round(w1Dur * 1.0));

  const tA = (startTime + durationA) as UTCTimestamp;
  const tB = (startTime + durationA + durationB) as UTCTimestamp;
  const tC = (startTime + durationA + durationB + durationC) as UTCTimestamp;

  const aPt: FormationPoint = { time: tA, value: projAVal };
  const bPt: FormationPoint = { time: tB, value: projBVal };
  const cPt: FormationPoint = { time: tC, value: projCVal };

  const postGuide = postImpulseStructuralGuide(postAbc);
  const hasA = !!postGuide?.hasA;
  const hasB = !!postGuide?.hasB;
  const hasC = !!postGuide?.hasC;

  const w5Pt: FormationPoint = { time: p5.time as UTCTimestamp, value: p5.price };

  if (!hasA) {
    // Impulse done -> project full ABC
    layers.push(seg(w5Pt, aPt, kind, "dashed", lineColor));
    layers.push(seg(aPt, bPt, kind, "dotted", lineColor));
    layers.push(seg(bPt, cPt, kind, "dashed", lineColor));
  } else if (hasA && !hasB) {
    // A observed -> project B and C from observed A point
    const obsA = postGuide!.a;
    const obsAPt: FormationPoint = { time: obsA.time as UTCTimestamp, value: obsA.price };
    const abObs = Math.abs(obsAPt.value - p5.price);
    const projB2 = isBull ? obsAPt.value + abObs * 0.618 : obsAPt.value - abObs * 0.618;
    const projC2 = isBull ? projB2 - abObs * 1.0 : projB2 + abObs * 1.0;
    const b2: FormationPoint = { time: tB, value: projB2 };
    const c2: FormationPoint = { time: tC, value: projC2 };
    layers.push(seg(obsAPt, b2, kind, "dashed", lineColor));
    layers.push(seg(b2, c2, kind, "dashed", lineColor));
  } else if (hasA && hasB && !hasC) {
    // A and B observed -> project C from observed B
    const obsB = postGuide!.b;
    const obsBPt: FormationPoint = { time: obsB.time as UTCTimestamp, value: obsB.price };
    const aRef = postGuide!.a;
    const abObs = Math.abs(aRef.price - p5.price);
    const projC2 = isBull ? obsBPt.value - abObs * 1.0 : obsBPt.value + abObs * 1.0;
    const c2: FormationPoint = { time: tC, value: projC2 };
    layers.push(seg(obsBPt, c2, kind, "dashed", lineColor));
  } else {
    // ABC completed -> project new impulse 1-2-3-4-5 from C end (observed)
    const cEnd = postGuide!.patternEnd;
    const cEndPt: FormationPoint = { time: cEnd.time as UTCTimestamp, value: cEnd.price };
    const w3Target = cEndPt.value + dir * w1Size * 1.618;
    const w4Target = w3Target - dir * Math.abs(w3Target - cEndPt.value) * 0.382;
    const w5Target = w4Target + dir * w1Size * 1.0;
    const t1 = (cEndPt.time as number) + Math.max(60, Math.round(w1Dur * 1.0));
    const t2 = t1 + Math.max(60, Math.round(w2Dur * 1.0));
    const t3 = t2 + Math.max(60, Math.round(w3Dur * 1.0));
    const t4 = t3 + Math.max(60, Math.round(w2Dur * 1.0));
    const t5 = t4 + Math.max(60, Math.round(w1Dur * 1.0));
    const p1n: FormationPoint = { time: t1 as UTCTimestamp, value: cEndPt.value + dir * w1Size * 1.0 };
    const p2n: FormationPoint = { time: t2 as UTCTimestamp, value: p1n.value - dir * w1Size * 0.382 };
    const p3n: FormationPoint = { time: t3 as UTCTimestamp, value: w3Target };
    const p4n: FormationPoint = { time: t4 as UTCTimestamp, value: w4Target };
    const p5n: FormationPoint = { time: t5 as UTCTimestamp, value: w5Target };
    layers.push(seg(cEndPt, p1n, kind, "dashed", lineColor));
    layers.push(seg(p1n, p2n, kind, "dashed", lineColor));
    layers.push(seg(p2n, p3n, kind, "dashed", lineColor));
    layers.push(seg(p3n, p4n, kind, "dashed", lineColor));
    layers.push(seg(p4n, p5n, kind, "dashed", lineColor));
  }

  // Target level horizontals + markers for the latest projected points (A/B/C)
  const toT = ((tC as number) + 3600) as UTCTimestamp;
  if (!hasC) {
    layers.push(horizLevel(startTime, toT as number, aPt.value, targetKind, lineColor));
    layers.push(horizLevel(startTime, toT as number, bPt.value, targetKind, lineColor));
    layers.push(horizLevel(startTime, toT as number, cPt.value, targetKind, lineColor));
  }

  return { layers };
}

const MULTI_SCENARIO_COLORS = ["#E57373", "#42A5F5", "#66BB6A", "#FFB74D", "#AB47BC"] as const;

function mergeProjectionPatternMenu(m?: ElliottPatternMenuToggles): ElliottPatternMenuToggles {
  return { ...DEFAULT_ELLIOTT_PATTERN_MENU, ...m };
}

function layerSegmentsBetweenPivots(
  pivots: ZigzagPivot[],
  color: string,
  style: "solid" | "dashed" | "dotted",
): PatternLayerOverlay[] {
  const out: PatternLayerOverlay[] = [];
  for (let i = 0; i < pivots.length - 1; i++) {
    const a = { time: pivots[i]!.time as UTCTimestamp, value: pivots[i]!.price };
    const b = { time: pivots[i + 1]!.time as UTCTimestamp, value: pivots[i + 1]!.price };
    out.push({
      upper: [],
      lower: [],
      zigzag: [a, b],
      zigzagKind: "elliott_projection",
      zigzagLineColor: color,
      zigzagLineStyle: style,
    });
  }
  return out;
}

/** Theoretical B/C after observed A (zigzag: moderate B retrace; flat: deep B). */
function theoreticalBCPrices(
  impulseBull: boolean,
  p5: ZigzagPivot,
  obsA: ZigzagPivot,
  pattern: "zigzag" | "flat",
): { b: number; c: number } {
  const bRetrace = pattern === "zigzag" ? 0.5 : 0.92;
  const cMul = pattern === "zigzag" ? 1.0 : 0.88;
  if (impulseBull) {
    const legA = p5.price - obsA.price;
    if (legA <= 1e-12) return { b: obsA.price, c: obsA.price };
    const b = obsA.price + legA * bRetrace;
    const c = b - Math.abs(b - obsA.price) * cMul;
    return { b, c };
  }
  const legA = obsA.price - p5.price;
  if (legA <= 1e-12) return { b: obsA.price, c: obsA.price };
  const b = obsA.price - legA * bRetrace;
  const c = b + Math.abs(b - obsA.price) * cMul;
  return { b, c };
}

/**
 * Formation projection: enumerate zigzag + flat (and pivot-backed ABC candidates), separate colors.
 * Returns [] when ABC is complete (caller uses single `buildFormationProjection` for the next impulse leg).
 */
function buildMultiScenarioFormationLayers(params: {
  imp: ImpulseCountV2;
  postAbc: CorrectiveCountV2 | null;
  zigzagPivots: ZigzagPivot[];
  startTime: number;
  lineColor?: string;
  patternMenu: ElliottPatternMenuToggles;
}): PatternLayerOverlay[] {
  const menu = mergeProjectionPatternMenu(params.patternMenu);
  const allowZig = patternMenuAllowsZigzagAbc(menu);
  const allowFlat = patternMenuAllowsFlatAbc(menu);
  if (!allowZig && !allowFlat) return [];

  const { imp, postAbc, zigzagPivots, startTime } = params;
  const p = imp.pivots;
  const p5 = p[5];
  const isBull = imp.direction === "bull";
  const postGuide = postImpulseStructuralGuide(postAbc);
  const hasA = !!postGuide?.hasA;
  const hasB = !!postGuide?.hasB;
  const hasC = !!postGuide?.hasC;

  const w1Dur = Math.max(60, p[1].time - p[0].time);
  const w2Dur = Math.max(60, p[2].time - p[1].time);
  const minSegDur = Math.max(120, Math.round(w1Dur * 0.35));
  const durationB = Math.max(minSegDur, Math.round(w2Dur * 1.0));
  const durationC = Math.max(minSegDur, Math.round(w1Dur * 1.0));

  let cidx = 0;
  const nextColor = () => MULTI_SCENARIO_COLORS[cidx++ % MULTI_SCENARIO_COLORS.length]!;

  const layers: PatternLayerOverlay[] = [];

  if (hasC) {
    return [];
  }

  if (hasB) {
    const obsA = postGuide!.a;
    const obsB = postGuide!.b;
    const fixed = collectAbcCorrectivesWithFixedAB(imp, obsA, obsB, zigzagPivots, menu);
    if (fixed.length) {
      for (const c of fixed) {
        const col = nextColor();
        const cEnd = c.pivots[3]!;
        layers.push(...layerSegmentsBetweenPivots([obsB, cEnd], col, "dashed"));
      }
      return layers;
    }
    for (const pat of ["zigzag", "flat"] as const) {
      if (pat === "zigzag" && !allowZig) continue;
      if (pat === "flat" && !allowFlat) continue;
      const th = theoreticalBCPrices(isBull, p5, obsA, pat);
      const cVal = isBull ? obsB.price - Math.abs(th.b - th.c) : obsB.price + Math.abs(th.b - th.c);
      const tC = (obsB.time + durationC) as UTCTimestamp;
      const col = nextColor();
      layers.push({
        upper: [],
        lower: [],
        zigzag: [
          { time: obsB.time as UTCTimestamp, value: obsB.price },
          { time: tC, value: cVal },
        ],
        zigzagKind: "elliott_projection",
        zigzagLineColor: col,
        zigzagLineStyle: "dashed",
      });
    }
    return layers;
  }

  if (hasA) {
    const obsA = postGuide!.a;
    const tAfterA = Math.max(obsA.time, startTime);
    for (const pat of ["zigzag", "flat"] as const) {
      if (pat === "zigzag" && !allowZig) continue;
      if (pat === "flat" && !allowFlat) continue;
      const { b, c } = theoreticalBCPrices(isBull, p5, obsA, pat);
      const tB = (tAfterA + durationB) as UTCTimestamp;
      const tC = (tAfterA + durationB + durationC) as UTCTimestamp;
      const col = nextColor();
      const obsAPt = { time: obsA.time as UTCTimestamp, value: obsA.price };
      layers.push({
        upper: [],
        lower: [],
        zigzag: [obsAPt, { time: tB, value: b }],
        zigzagKind: "elliott_projection",
        zigzagLineColor: col,
        zigzagLineStyle: "dashed",
      });
      layers.push({
        upper: [],
        lower: [],
        zigzag: [{ time: tB, value: b }, { time: tC, value: c }],
        zigzagKind: "elliott_projection",
        zigzagLineColor: col,
        zigzagLineStyle: "dotted",
      });
    }
    return layers;
  }

  const cands = collectAllAbcPostImpulseCandidates(zigzagPivots, imp, menu);
  if (cands.length) {
    for (const c of cands) {
      const col = nextColor();
      const path = [c.pivots[0]!, c.pivots[1]!, c.pivots[2]!, c.pivots[3]!];
      layers.push(...layerSegmentsBetweenPivots(path, col, "solid"));
    }
    return layers;
  }

  const impulseSize = Math.abs(p5.price - p[0].price);
  const projAVal = isBull ? p5.price - impulseSize * 0.382 : p5.price + impulseSize * 0.382;
  const abSize = Math.abs(projAVal - p5.price);
  const durationA = Math.max(minSegDur, Math.round(w1Dur * 0.618));
  const tA = (startTime + durationA) as UTCTimestamp;
  const tBBase = startTime + durationA + durationB;
  const tCBase = startTime + durationA + durationB + durationC;

  for (const pat of ["zigzag", "flat"] as const) {
    if (pat === "zigzag" && !allowZig) continue;
    if (pat === "flat" && !allowFlat) continue;
    const bRetrace = pat === "zigzag" ? 0.618 : 0.9;
    const cMul = pat === "zigzag" ? 1.0 : 0.85;
    const projBVal = isBull ? projAVal + abSize * bRetrace : projAVal - abSize * bRetrace;
    const projCVal = isBull ? projBVal - abSize * cMul : projBVal + abSize * cMul;
    const col = nextColor();
    const w5Pt = { time: p5.time as UTCTimestamp, value: p5.price };
    layers.push({
      upper: [],
      lower: [],
      zigzag: [w5Pt, { time: tA, value: projAVal }],
      zigzagKind: "elliott_projection",
      zigzagLineColor: col,
      zigzagLineStyle: "dashed",
    });
    layers.push({
      upper: [],
      lower: [],
      zigzag: [{ time: tA, value: projAVal }, { time: tBBase as UTCTimestamp, value: projBVal }],
      zigzagKind: "elliott_projection",
      zigzagLineColor: col,
      zigzagLineStyle: "dotted",
    });
    layers.push({
      upper: [],
      lower: [],
      zigzag: [{ time: tBBase as UTCTimestamp, value: projBVal }, { time: tCBase as UTCTimestamp, value: projCVal }],
      zigzagKind: "elliott_projection",
      zigzagLineColor: col,
      zigzagLineStyle: "dashed",
    });
  }
  return layers;
}

/**
 * V2 formation-only forward projection (ABC / next impulse segments).
 * Anchor: last bar of `ohlcByTf[sourceTf]` when present, else `anchorRows`.
 */
export function buildElliottProjectionOverlayV2(
  out: ElliottEngineOutputV2,
  anchorRows: OhlcV2[],
  opt: ElliottProjectionV2Options,
  patternMenu?: ElliottPatternMenuToggles,
  lineColor?: string,
  sourceTf?: Timeframe,
): { layers: PatternLayerOverlay[] } | null {
  const tf = sourceTf ?? "1h";
  const imp = sourceTf ? impulseForProjectionTf(out, sourceTf) : chooseImpulse(out);
  if (!imp || !impulseShownInMenu(imp, patternMenu) || anchorRows.length < 2) return null;
  const postAbc = sourceTf ? postAbcForProjectionTf(out, sourceTf) : out.hierarchy.intermediate?.postImpulseAbc ?? null;

  const rowsForStep =
    sourceTf && out.ohlcByTf?.[sourceTf]?.length ? out.ohlcByTf[sourceTf]! : anchorRows;

  const p = imp.pivots;
  const p5 = p[5];
  const isBull = imp.direction === "bull";

  const base = blendedProjectionPriceBase(imp, rowsForStep);

  const anchorLast = rowsForStep.length ? rowsForStep[rowsForStep.length - 1]! : anchorRows[anchorRows.length - 1]!;
  const startPrice = anchorLast.c;
  const startTime = anchorLast.t;
  const postGuide = postImpulseStructuralGuide(postAbc);

  const layers: PatternLayerOverlay[] = [];
  const showAlt = opt.includeAltScenario !== false;
  /** When structural C/Y exists and last close confirms trend resumption, skip synthetic next-impulse paths. */
  let skipForwardFormationAfterConfirmedCorrection = false;

  if (postGuide?.hasB) {
    const { a, b, c, hasC } = postGuide;
    const dir = isBull ? 1 : -1;
    const cCompleted = !!c && startPrice * dir > c.price * dir + 0.05 * base;
    if (cCompleted) skipForwardFormationAfterConfirmedCorrection = true;
    const donePts: Pt[] = [
      { time: p5.time as UTCTimestamp, value: p5.price },
      { time: a.time as UTCTimestamp, value: a.price },
      { time: b.time as UTCTimestamp, value: b.price },
    ];
    if (c && cCompleted) donePts.push({ time: c.time as UTCTimestamp, value: c.price });
    layers.push({
      upper: [],
      lower: [],
      zigzag: donePts,
      zigzagKind: "elliott_projection_done",
      zigzagLineColor: lineColor,
    });
    if (!cCompleted) {
      const anchor = hasC && c ? c : b;
      layers.push({
        upper: [],
        lower: [],
        zigzag: [
          { time: anchor.time as UTCTimestamp, value: anchor.price },
          { time: startTime as UTCTimestamp, value: startPrice },
        ],
        zigzagKind: "elliott_projection_c_active",
        zigzagLineColor: lineColor,
      });
    }
  }

  const zzPivots =
    (sourceTf ? out.states[sourceTf]?.pivots : undefined) ?? out.states[tf]?.pivots ?? [];
  const multi =
    !skipForwardFormationAfterConfirmedCorrection &&
    opt.multiCorrectiveScenarios &&
    patternMenu
      ? buildMultiScenarioFormationLayers({
          imp,
          postAbc,
          zigzagPivots: zzPivots,
          startTime,
          lineColor,
          patternMenu,
        })
      : [];
  if (multi.length) {
    layers.push(...multi);
  } else if (!skipForwardFormationAfterConfirmedCorrection) {
    const formed = buildFormationProjection({
      imp,
      postAbc,
      startTime,
      startPrice,
      tf,
      lineColor,
      alt: false,
    });
    layers.push(...formed.layers);
    if (showAlt) {
      const formedAlt = buildFormationProjection({
        imp,
        postAbc,
        startTime,
        startPrice,
        tf,
        lineColor,
        alt: true,
      });
      layers.push(...formedAlt.layers);
    }
  }

  return { layers };
}

