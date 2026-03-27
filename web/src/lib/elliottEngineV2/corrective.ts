import type { CorrectiveCountV2, ElliottRuleCheckV2, ImpulseCountV2, ZigzagPivot } from "./types";
import { buildTez254AbcChecks } from "./tezWaveChecks";
import { DEFAULT_ELLIOTT_PATTERN_MENU, type ElliottPatternMenuToggles } from "../elliottPatternMenuCatalog";

function mergePatternToggles(t?: ElliottPatternMenuToggles): ElliottPatternMenuToggles {
  return { ...DEFAULT_ELLIOTT_PATTERN_MENU, ...t };
}

const EPS = 1e-10;
type CorrectiveContext = "wave2" | "wave4" | "post";

/** Yassı: B, A’nın büyük bölümünü geri alır; değilse zigzag adayı (§2.5.4.1 / 2.5.4.2). */
function classifyFlatVsZigzag(retrB: number): "flat" | "zigzag" {
  return retrB >= 0.9 ? "flat" : "zigzag";
}

function collectAbcCorrectiveDown(
  start: ZigzagPivot,
  end: ZigzagPivot,
  inner: ZigzagPivot[],
): CorrectiveCountV2 | null {
  const candidates: CorrectiveCountV2[] = [];
  const impulseBull = true;

  for (const a of inner) {
    if (a.kind !== "low") continue;
    if (a.price >= start.price) continue;
    for (const b of inner) {
      if (b.kind !== "high") continue;
      if (b.index <= a.index || b.index >= end.index) continue;
      if (b.price <= a.price || b.price >= start.price + EPS) continue;

      const lenA = start.price - a.price;
      if (lenA <= EPS) continue;
      const retrB = (b.price - a.price) / lenA;
      const lenC = a.price - end.price;
      const cVsA = lenA > EPS ? lenC / lenA : 0;
      if (retrB < 0.12 || retrB > 2.85) continue;

      const pattern = classifyFlatVsZigzag(retrB);
      const baseChecks: ElliottRuleCheckV2[] = [
        { id: "abc_order", passed: start.index < a.index && a.index < b.index && b.index < end.index },
        { id: "abc_b_retrace", passed: retrB >= 0.15 && retrB <= 2.8, detail: retrB.toFixed(3) },
        { id: "abc_c_extent", passed: cVsA >= 0.3 && cVsA <= 2.4, detail: cVsA.toFixed(3) },
      ];
      const tezChecks = buildTez254AbcChecks(pattern, retrB, cVsA, impulseBull, start, a, b, end);
      const checks = [...baseChecks, ...tezChecks];
      const score = checks.filter((x) => x.passed).length + (pattern === "flat" ? 0.2 : 0.3);
      candidates.push({ pivots: [start, a, b, end], pattern, checks, score, labels: ["a", "b", "c"] });
    }
  }
  if (!candidates.length) return null;
  candidates.sort((a, b) => b.score - a.score);
  return candidates[0]!;
}

function collectAbcCorrectiveUp(
  start: ZigzagPivot,
  end: ZigzagPivot,
  inner: ZigzagPivot[],
): CorrectiveCountV2 | null {
  const candidates: CorrectiveCountV2[] = [];
  const impulseBull = false;

  for (const a of inner) {
    if (a.kind !== "high") continue;
    if (a.price <= start.price) continue;
    for (const b of inner) {
      if (b.kind !== "low") continue;
      if (b.index <= a.index || b.index >= end.index) continue;
      if (b.price >= a.price || b.price <= start.price - EPS) continue;

      const lenA = a.price - start.price;
      if (lenA <= EPS) continue;
      const retrB = (a.price - b.price) / lenA;
      const lenC = end.price - b.price;
      const cVsA = lenA > EPS ? lenC / lenA : 0;
      if (retrB < 0.12 || retrB > 2.85) continue;

      const pattern = classifyFlatVsZigzag(retrB);
      const baseChecks: ElliottRuleCheckV2[] = [
        { id: "abc_order", passed: start.index < a.index && a.index < b.index && b.index < end.index },
        { id: "abc_b_retrace", passed: retrB >= 0.15 && retrB <= 2.8, detail: retrB.toFixed(3) },
        { id: "abc_c_extent", passed: cVsA >= 0.3 && cVsA <= 2.4, detail: cVsA.toFixed(3) },
      ];
      const tezChecks = buildTez254AbcChecks(pattern, retrB, cVsA, impulseBull, start, a, b, end);
      const checks = [...baseChecks, ...tezChecks];
      const score = checks.filter((x) => x.passed).length + (pattern === "flat" ? 0.2 : 0.3);
      candidates.push({ pivots: [start, a, b, end], pattern, checks, score, labels: ["a", "b", "c"] });
    }
  }
  if (!candidates.length) return null;
  candidates.sort((a, b) => b.score - a.score);
  return candidates[0]!;
}

function findCorrectiveBetween(
  pivots: ZigzagPivot[],
  start: ZigzagPivot,
  end: ZigzagPivot,
  direction: "down" | "up",
  context: CorrectiveContext,
  toggles?: ElliottPatternMenuToggles,
): CorrectiveCountV2 | null {
  const t = mergePatternToggles(toggles);
  const inner = pivots.filter((p) => p.index > start.index && p.index < end.index);
  if (inner.length < 2) return null;

  const tryAbc = (): CorrectiveCountV2 | null => {
    if (!t.corrective_zigzag && !t.corrective_flat) return null;
    const abc = direction === "down" ? collectAbcCorrectiveDown(start, end, inner) : collectAbcCorrectiveUp(start, end, inner);
    if (!abc) return null;
    if (abc.pattern === "zigzag" && !t.corrective_zigzag) return null;
    if (abc.pattern === "flat" && !t.corrective_flat) return null;
    return abc;
  };

  if (direction === "down") {
    const abc = tryAbc();
    if (abc) return abc;
    if (t.corrective_triangle) {
      const tri = findTriangleBetween(start, end, inner, context);
      if (tri) return tri;
    }
    if (t.corrective_complex_wxy) {
      const wxyxz = findWxyxzBetween(start, end, inner, "down", context);
      if (wxyxz) return wxyxz;
      const comb = findCombinationBetween(start, end, inner, "down");
      if (comb) return comb;
    }
    return null;
  }

  const abc = tryAbc();
  if (abc) return abc;
  if (t.corrective_triangle) {
    const tri = findTriangleBetween(start, end, inner, context);
    if (tri) return tri;
  }
  if (t.corrective_complex_wxy) {
    const wxyxz = findWxyxzBetween(start, end, inner, "up", context);
    if (wxyxz) return wxyxz;
    return findCombinationBetween(start, end, inner, "up");
  }
  return null;
}

function findTriangleBetween(
  start: ZigzagPivot,
  end: ZigzagPivot,
  inner: ZigzagPivot[],
  context: CorrectiveContext,
): CorrectiveCountV2 | null {
  // Triangle in Elliott context is typically wave 4 or B-like region.
  if (context !== "wave4" && context !== "post") return null;
  if (inner.length < 3) return null;
  const seq = [start, ...inner, end];
  const last = seq.slice(-5);
  if (last.length < 5) return null;

  const alternating = last.every((p, i) => (i === 0 ? true : p.kind !== last[i - 1].kind));
  if (!alternating) return null;

  const highs = last.filter((p) => p.kind === "high");
  const lows = last.filter((p) => p.kind === "low");
  if (highs.length < 2 || lows.length < 2) return null;

  const span1 = Math.abs(last[1].price - last[0].price);
  const span2 = Math.abs(last[2].price - last[1].price);
  const span3 = Math.abs(last[3].price - last[2].price);
  const span4 = Math.abs(last[4].price - last[3].price);
  const width = Math.max(span1, span2, span3, span4);
  if (!Number.isFinite(width) || width <= EPS) return null;
  const tightening = span4 <= span1 * 0.85 || span4 <= span2 * 0.85;

  // Geometric channel constraint (contracting wedge):
  // high-line should slope down or stay flat; low-line should slope up or stay flat.
  const hSlope = highs[highs.length - 1].price - highs[0].price;
  const lSlope = lows[lows.length - 1].price - lows[0].price;
  const converging = hSlope <= EPS && lSlope >= -EPS;

  const envelope0 = Math.abs(highs[0].price - lows[0].price);
  const envelopeN = Math.abs(highs[highs.length - 1].price - lows[lows.length - 1].price);
  const envelopeContract = envelopeN <= envelope0 * 0.9;

  const legA = Math.abs(last[1].price - last[0].price);
  const legB = Math.abs(last[2].price - last[1].price);
  const bVsA = legA > EPS ? legB / legA : 0;
  /** §2.5.4.3 tri_r5 — B, A’nın %38.2–%161.8 aralığında */
  const triR5 = bVsA >= 0.382 && bVsA <= 1.618;

  const checks: ElliottRuleCheckV2[] = [
    { id: "triangle_alt", passed: alternating },
    {
      id: "triangle_tightening",
      passed: tightening,
      detail: `${span4.toFixed(3)}<=${Math.max(span1, span2).toFixed(3)}`,
    },
    { id: "triangle_converging", passed: converging, detail: `hSlope=${hSlope.toFixed(3)} lSlope=${lSlope.toFixed(3)}` },
    { id: "triangle_envelope_contract", passed: envelopeContract, detail: `${envelopeN.toFixed(3)}<=${(envelope0 * 0.9).toFixed(3)}` },
    { id: "tri_r5", passed: triR5, detail: `B/A=${bVsA.toFixed(3)} ∈ [0.382,1.618]` },
    { id: "triangle_context_wave4_or_b", passed: true, detail: context },
  ];
  const score = checks.filter((x) => x.passed).length + 0.2;
  if (!converging || !envelopeContract) return null;
  return {
    pivots: [start, last[1], last[2], end],
    path: last,
    labels: ["a", "b", "c", "d", "e"],
    pattern: "triangle",
    checks,
    score,
  };
}

function findCombinationBetween(
  start: ZigzagPivot,
  end: ZigzagPivot,
  inner: ZigzagPivot[],
  direction: "down" | "up",
): CorrectiveCountV2 | null {
  if (inner.length < 4) return null;
  const seq = [start, ...inner, end];
  const alternating = seq.every((p, i) => (i === 0 ? true : p.kind !== seq[i - 1].kind));
  if (!alternating) return null;
  const first = seq[0].price;
  const last = seq[seq.length - 1].price;
  const progressed = direction === "down" ? last < first - EPS : last > first + EPS;
  if (!progressed) return null;

  const checks: ElliottRuleCheckV2[] = [
    { id: "comb_alt", passed: alternating },
    { id: "comb_progress", passed: progressed },
  ];
  const score = checks.filter((x) => x.passed).length + 0.05;
  return {
    pivots: [start, seq[1], seq[2], end],
    path: seq,
    labels: ["w", "x", "y"],
    pattern: "combination",
    checks,
    score,
  };
}

function findWxyxzBetween(
  start: ZigzagPivot,
  end: ZigzagPivot,
  inner: ZigzagPivot[],
  direction: "down" | "up",
  context: CorrectiveContext,
): CorrectiveCountV2 | null {
  if (inner.length < 4) return null;
  const seq = [start, ...inner, end];
  const last = seq.slice(-6);
  if (last.length < 6) return null;

  const alternating = last.every((p, i) => (i === 0 ? true : p.kind !== last[i - 1].kind));
  if (!alternating) return null;

  const first = last[0].price;
  const final = last[last.length - 1].price;
  const progressed = direction === "down" ? final < first - EPS : final > first + EPS;
  if (!progressed) return null;

  const legW = Math.abs(last[1].price - last[0].price);
  const legY = Math.abs(last[3].price - last[2].price);
  const legZ = Math.abs(last[5].price - last[4].price);
  const x1 = Math.abs(last[2].price - last[1].price);
  const x2 = Math.abs(last[4].price - last[3].price);
  const connectorsReasonable = x1 <= legW * 0.95 && x2 <= Math.max(legY, legW) * 0.95;

  // Requested ratio bands:
  // W≈Y (within 0.618..1.618), Z as extension of Y (0.618..2.0).
  const yVsW = legW > EPS ? legY / legW : 0;
  const zVsY = legY > EPS ? legZ / legY : 0;
  const ratioWApproxY = yVsW >= 0.618 && yVsW <= 1.618;
  const zExtensionBand = zVsY >= 0.618 && zVsY <= 2.0;

  // X retrace bands over previous trend legs (23.6%..78.6%).
  const x1VsW = legW > EPS ? x1 / legW : 0;
  const x2VsY = legY > EPS ? x2 / legY : 0;
  const xRetraceBand = x1VsW >= 0.236 && x1VsW <= 0.786 && x2VsY >= 0.236 && x2VsY <= 0.786;

  // Post-B context gate (requested).
  const postBContext = context === "post";

  const checks: ElliottRuleCheckV2[] = [
    { id: "wxyxz_alt", passed: alternating },
    { id: "wxyxz_progress", passed: progressed },
    { id: "wxyxz_x_connectors", passed: connectorsReasonable, detail: `x1=${x1.toFixed(3)} x2=${x2.toFixed(3)}` },
    { id: "wxyxz_ratio_wy", passed: ratioWApproxY, detail: `Y/W=${yVsW.toFixed(3)}` },
    { id: "wxyxz_ratio_zy", passed: zExtensionBand, detail: `Z/Y=${zVsY.toFixed(3)}` },
    { id: "wxyxz_x_retrace_band", passed: xRetraceBand, detail: `x1/W=${x1VsW.toFixed(3)} x2/Y=${x2VsY.toFixed(3)}` },
    { id: "wxyxz_post_b_context", passed: postBContext, detail: context },
  ];
  const confirmed =
    connectorsReasonable &&
    ratioWApproxY &&
    zExtensionBand &&
    xRetraceBand &&
    postBContext;
  checks.push({ id: "wxyxz_confirmed", passed: confirmed, detail: confirmed ? "confirmed" : "candidate" });
  const score = checks.filter((x) => x.passed).length + (confirmed ? 0.45 : 0.12);
  // keep candidate, but reject if base structure is very weak
  if (!connectorsReasonable || !alternating || !progressed) return null;

  return {
    pivots: [last[0], last[1], last[2], last[5]],
    path: last,
    labels: ["w", "x", "y", "x", "z"],
    pattern: "combination",
    checks,
    score,
  };
}

export function detectImpulseCorrectionsV2(
  pivots: ZigzagPivot[],
  impulse: ImpulseCountV2,
  patternToggles?: ElliottPatternMenuToggles,
): { wave2: CorrectiveCountV2 | null; wave4: CorrectiveCountV2 | null; postImpulseAbc: CorrectiveCountV2 | null } {
  const [, p1, p2, p3, p4, p5] = impulse.pivots;
  const isBull = impulse.direction === "bull";
  const wave2 = findCorrectiveBetween(pivots, p1, p2, isBull ? "down" : "up", "wave2", patternToggles);
  const wave4 = findCorrectiveBetween(pivots, p3, p4, isBull ? "down" : "up", "wave4", patternToggles);

  let postImpulseAbc: CorrectiveCountV2 | null = null;
  const later = pivots.filter((p) => p.index > p5.index);
  if (later.length) {
    const end = later[later.length - 1];
    postImpulseAbc = findCorrectiveBetween(pivots, p5, end, isBull ? "down" : "up", "post", patternToggles);
  }
  return { wave2, wave4, postImpulseAbc };
}

