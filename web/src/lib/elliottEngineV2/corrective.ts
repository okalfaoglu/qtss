import type {
  CorrectiveCountV2,
  CorrectivePatternV2,
  ElliottRuleCheckV2,
  ImpulseCountV2,
  OhlcV2,
  ZigzagParams,
  ZigzagPivot,
} from "./types";
import { microDepthCandidatesForNestedLeg } from "./impulse";
import { buildZigzagPivotsV2 } from "./zigzag";
import {
  buildTez254AbcChecks,
  TEZ_FLAT_B_VS_A_MAX,
  TEZ_FLAT_B_VS_A_MIN,
  TEZ_FLAT_LABEL_MIN_RETR_B,
  TEZ_ZIGZAG_B_VS_A_MAX,
} from "./tezWaveChecks";
import { DEFAULT_ELLIOTT_PATTERN_MENU, type ElliottPatternMenuToggles } from "../elliottPatternMenuCatalog";

function mergePatternToggles(t?: ElliottPatternMenuToggles): ElliottPatternMenuToggles {
  return { ...DEFAULT_ELLIOTT_PATTERN_MENU, ...t };
}

export function patternMenuAllowsZigzagAbc(t: ElliottPatternMenuToggles): boolean {
  return t.corrective_zigzag;
}

export function patternMenuAllowsFlatAbc(t: ElliottPatternMenuToggles): boolean {
  return t.corrective_flat;
}

const EPS = 1e-10;
type CorrectiveContext = "wave2" | "wave4" | "post";

/** §2.5.4.3 tri_r2 — A–C ve B–D çizgilerinde linear fiyat (index = zaman). */
function linearPriceAtIndex(t: number, t1: number, p1: number, t2: number, p2: number): number {
  if (Math.abs(t2 - t1) < EPS) return p1;
  return p1 + ((p2 - p1) * (t - t1)) / (t2 - t1);
}

/** İki doğrunun (index, fiyat) kesişim x’i; paralel → null */
function lineLineIntersectIndexX(
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  x3: number,
  y3: number,
  x4: number,
  y4: number,
): number | null {
  const denom = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
  if (Math.abs(denom) < 1e-12) return null;
  const t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / denom;
  return x1 + t * (x2 - x1);
}

/**
 * §2.5.4.3 — 6 pivot: A–C ve B–D uçları; tüm tepe/dipler iki çizgi arasında mı?
 */
function triangleTriR2ChannelOk(pts: ZigzagPivot[]): boolean {
  if (pts.length < 6) return false;
  const p = pts;
  for (let i = 0; i < 6; i++) {
    const t = p[i].index;
    const vAC = linearPriceAtIndex(t, p[1].index, p[1].price, p[3].index, p[3].price);
    const vBD = linearPriceAtIndex(t, p[2].index, p[2].price, p[4].index, p[4].price);
    const lo = Math.min(vAC, vBD);
    const hi = Math.max(vAC, vBD);
    if (p[i].price < lo - 1e-7 || p[i].price > hi + 1e-7) return false;
  }
  return true;
}

/** tri_r3 — üst/alt bant kesişimi E bitişinden sonra (gelecekteki apex). */
function triangleTriR3ApexAfterE(pts: ZigzagPivot[]): boolean {
  if (pts.length < 6) return false;
  const p = pts;
  const ix = lineLineIntersectIndexX(
    p[1].index,
    p[1].price,
    p[3].index,
    p[3].price,
    p[2].index,
    p[2].price,
    p[4].index,
    p[4].price,
  );
  if (ix === null) return false;
  return ix + EPS >= p[5].index;
}

/** tri_r4 — çizgiler paralel değil (eğim farkı). */
function triangleTriR4NotParallel(pts: ZigzagPivot[]): boolean {
  if (pts.length < 6) return false;
  const p = pts;
  const dx1 = p[3].index - p[1].index;
  const dy1 = p[3].price - p[1].price;
  const dx2 = p[4].index - p[2].index;
  const dy2 = p[4].price - p[2].price;
  if (Math.abs(dx1) < EPS || Math.abs(dx2) < EPS) return false;
  const s1 = dy1 / dx1;
  const s2 = dy2 / dx2;
  return Math.abs(s1 - s2) > 1e-9;
}

/** tri_r7 — genişleyen üçgende en kısa dalga A veya B (ilk iki bacak). */
function triangleTriR7ExpandingShortestAb(pts: ZigzagPivot[]): boolean {
  if (pts.length < 6) return false;
  const p = pts;
  const L = [
    Math.abs(p[1].price - p[0].price),
    Math.abs(p[2].price - p[1].price),
    Math.abs(p[3].price - p[2].price),
    Math.abs(p[4].price - p[3].price),
    Math.abs(p[5].price - p[4].price),
  ];
  const minL = Math.min(...L);
  const idx = L.indexOf(minL);
  return idx === 0 || idx === 1;
}

/** Flat etiketi için B/A tabanı (≈düzenli yassı); altı zigzag adayı. */
const FLAT_LABEL_MIN_RETR_B = 0.9;

/** Yassı: B, A’nın büyük bölümünü geri alır; değilse zigzag adayı (§2.5.4.1 / 2.5.4.2). */
function classifyFlatVsZigzag(retrB: number): "flat" | "zigzag" {
  return retrB >= FLAT_LABEL_MIN_RETR_B ? "flat" : "zigzag";
}

/** Tez B/A aralığı + zigzag/flat ayrımı: (0.618, 0.9) aralığında geçerli desen yok → erken çık. */
function abcRetrBPassesPrefilter(retrB: number): boolean {
  if (retrB < TEZ_FLAT_B_VS_A_MIN - 1e-12) return false;
  if (retrB > TEZ_FLAT_B_VS_A_MAX + 1e-9) return false;
  if (retrB > TEZ_ZIGZAG_B_VS_A_MAX + 1e-12 && retrB < TEZ_FLAT_LABEL_MIN_RETR_B - 1e-12) return false;
  return true;
}

function hasPassedCheck(checks: ElliottRuleCheckV2[], id: string): boolean {
  return checks.some((x) => x.id === id && x.passed);
}

function abcCandidateIsValid(pattern: "zigzag" | "flat", checks: ElliottRuleCheckV2[]): boolean {
  const baseOk =
    hasPassedCheck(checks, "abc_order") &&
    hasPassedCheck(checks, "abc_b_retrace") &&
    hasPassedCheck(checks, "abc_c_extent");
  if (!baseOk) return false;
  if (pattern === "zigzag") {
    return (
      hasPassedCheck(checks, "zz_r1") &&
      hasPassedCheck(checks, "zz_r5") &&
      hasPassedCheck(checks, "zz_r6") &&
      hasPassedCheck(checks, "zz_b_not_beyond_a_start")
    );
  }
  return hasPassedCheck(checks, "flat_r4") && hasPassedCheck(checks, "flat_g7");
}

/**
 * One ABC quadruple (start=a.k. wave-5 end, a,b,c pivots) for bull impulse / down correction.
 */
function abcFromQuadrupleDown(
  start: ZigzagPivot,
  a: ZigzagPivot,
  b: ZigzagPivot,
  end: ZigzagPivot,
): CorrectiveCountV2 | null {
  const impulseBull = true;
  if (a.kind !== "low" || b.kind !== "high" || end.kind !== "low") return null;
  if (!(start.index < a.index && a.index < b.index && b.index < end.index)) return null;
  if (a.price >= start.price) return null;
  if (b.price <= a.price || b.price >= start.price + EPS) return null;
  if (end.price >= b.price - EPS) return null;

  const lenA = start.price - a.price;
  if (lenA <= EPS) return null;
  const retrB = (b.price - a.price) / lenA;
  const lenC = b.price - end.price;
  const cVsA = lenA > EPS ? lenC / lenA : 0;
  if (!abcRetrBPassesPrefilter(retrB)) return null;

  const pattern = classifyFlatVsZigzag(retrB);
  const baseChecks: ElliottRuleCheckV2[] = [
    { id: "abc_order", passed: true },
    {
      id: "abc_b_retrace",
      passed: retrB >= TEZ_FLAT_B_VS_A_MIN - 1e-12 && retrB <= TEZ_FLAT_B_VS_A_MAX + 1e-12,
      detail: retrB.toFixed(3),
    },
    { id: "abc_c_extent", passed: cVsA >= 0.3 && cVsA <= 2.4, detail: cVsA.toFixed(3) },
  ];
  const tezChecks = buildTez254AbcChecks(pattern, retrB, cVsA, impulseBull, start, a, b, end);
  const checks = [...baseChecks, ...tezChecks];
  if (!abcCandidateIsValid(pattern, checks)) return null;
  const score = checks.filter((x) => x.passed).length + (pattern === "flat" ? 0.2 : 0.3);
  return { pivots: [start, a, b, end], pattern, checks, score, labels: ["a", "b", "c"] };
}

function collectAllAbcCorrectiveDown(start: ZigzagPivot, end: ZigzagPivot, inner: ZigzagPivot[]): CorrectiveCountV2[] {
  const candidates: CorrectiveCountV2[] = [];
  for (const a of inner) {
    if (a.kind !== "low") continue;
    if (a.price >= start.price) continue;
    for (const b of inner) {
      if (b.kind !== "high") continue;
      if (b.index <= a.index || b.index >= end.index) continue;
      if (b.price <= a.price || b.price >= start.price + EPS) continue;
      const hit = abcFromQuadrupleDown(start, a, b, end);
      if (hit) candidates.push(hit);
    }
  }
  candidates.sort((x, y) => y.score - x.score);
  return candidates;
}

function collectAbcCorrectiveDown(
  start: ZigzagPivot,
  end: ZigzagPivot,
  inner: ZigzagPivot[],
): CorrectiveCountV2 | null {
  const candidates = collectAllAbcCorrectiveDown(start, end, inner);
  return candidates[0] ?? null;
}

function abcFromQuadrupleUp(
  start: ZigzagPivot,
  a: ZigzagPivot,
  b: ZigzagPivot,
  end: ZigzagPivot,
): CorrectiveCountV2 | null {
  const impulseBull = false;
  if (a.kind !== "high" || b.kind !== "low" || end.kind !== "high") return null;
  if (!(start.index < a.index && a.index < b.index && b.index < end.index)) return null;
  if (a.price <= start.price) return null;
  if (b.price >= a.price || b.price <= start.price - EPS) return null;
  if (end.price <= b.price + EPS) return null;

  const lenA = a.price - start.price;
  if (lenA <= EPS) return null;
  const retrB = (a.price - b.price) / lenA;
  const lenC = end.price - b.price;
  const cVsA = lenA > EPS ? lenC / lenA : 0;
  if (!abcRetrBPassesPrefilter(retrB)) return null;

  const pattern = classifyFlatVsZigzag(retrB);
  const baseChecks: ElliottRuleCheckV2[] = [
    { id: "abc_order", passed: true },
    {
      id: "abc_b_retrace",
      passed: retrB >= TEZ_FLAT_B_VS_A_MIN - 1e-12 && retrB <= TEZ_FLAT_B_VS_A_MAX + 1e-12,
      detail: retrB.toFixed(3),
    },
    { id: "abc_c_extent", passed: cVsA >= 0.3 && cVsA <= 2.4, detail: cVsA.toFixed(3) },
  ];
  const tezChecks = buildTez254AbcChecks(pattern, retrB, cVsA, impulseBull, start, a, b, end);
  const checks = [...baseChecks, ...tezChecks];
  if (!abcCandidateIsValid(pattern, checks)) return null;
  const score = checks.filter((x) => x.passed).length + (pattern === "flat" ? 0.2 : 0.3);
  return { pivots: [start, a, b, end], pattern, checks, score, labels: ["a", "b", "c"] };
}

function collectAllAbcCorrectiveUp(start: ZigzagPivot, end: ZigzagPivot, inner: ZigzagPivot[]): CorrectiveCountV2[] {
  const candidates: CorrectiveCountV2[] = [];
  for (const a of inner) {
    if (a.kind !== "high") continue;
    if (a.price <= start.price) continue;
    for (const b of inner) {
      if (b.kind !== "low") continue;
      if (b.index <= a.index || b.index >= end.index) continue;
      if (b.price >= a.price || b.price <= start.price - EPS) continue;
      const hit = abcFromQuadrupleUp(start, a, b, end);
      if (hit) candidates.push(hit);
    }
  }
  candidates.sort((x, y) => y.score - x.score);
  return candidates;
}

function collectAbcCorrectiveUp(
  start: ZigzagPivot,
  end: ZigzagPivot,
  inner: ZigzagPivot[],
): CorrectiveCountV2 | null {
  const candidates = collectAllAbcCorrectiveUp(start, end, inner);
  return candidates[0] ?? null;
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
    if (!patternMenuAllowsZigzagAbc(t) && !patternMenuAllowsFlatAbc(t)) return null;
    const abc = direction === "down" ? collectAbcCorrectiveDown(start, end, inner) : collectAbcCorrectiveUp(start, end, inner);
    if (!abc) return null;
    if (abc.pattern === "zigzag" && !patternMenuAllowsZigzagAbc(t)) return null;
    if (abc.pattern === "flat" && !patternMenuAllowsFlatAbc(t)) return null;
    return abc;
  };

  if (direction === "down") {
    const abc = tryAbc();
    if (abc) return abc;
    if (t.corrective_triangle) {
      const tri = findTriangleBetween(start, end, inner, context);
      if (tri) return tri;
    }
    if (t.corrective_complex_triple) {
      const wxyxz = findWxyxzBetween(start, end, inner, "down", context);
      if (wxyxz) return wxyxz;
    }
    if (t.corrective_complex_double) {
      const comb = findCombinationBetween(start, end, inner, "down", context);
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
  if (t.corrective_complex_triple) {
    const wxyxz = findWxyxzBetween(start, end, inner, "up", context);
    if (wxyxz) return wxyxz;
  }
  if (t.corrective_complex_double) {
    return findCombinationBetween(start, end, inner, "up", context);
  }
  return null;
}

function findTriangleBetween(
  start: ZigzagPivot,
  end: ZigzagPivot,
  inner: ZigzagPivot[],
  context: CorrectiveContext,
): CorrectiveCountV2 | null {
  /** §2.5.4.3: 4. dalga yaygın; 2/B/post bağlamında da aranır (2 nadir). */
  if (context !== "wave2" && context !== "wave4" && context !== "post") return null;
  if (inner.length < 4) return null;

  const seq = [start, ...inner, end];
  /** 5 dalga a–e → 6 uç pivot (3-3-3-3-3 alt yapı zigzag’ta doğrulanmaz). */
  if (seq.length < 6) return null;
  const pts = seq.slice(-6);
  if (pts.length < 6) return null;

  const alternating = pts.every((p, i) => (i === 0 ? true : p.kind !== pts[i - 1].kind));
  if (!alternating) return null;

  const highs = pts.filter((p) => p.kind === "high");
  const lows = pts.filter((p) => p.kind === "low");
  if (highs.length < 2 || lows.length < 2) return null;

  const span1 = Math.abs(pts[1].price - pts[0].price);
  const span2 = Math.abs(pts[2].price - pts[1].price);
  const span3 = Math.abs(pts[3].price - pts[2].price);
  const span4 = Math.abs(pts[4].price - pts[3].price);
  const span5 = Math.abs(pts[5].price - pts[4].price);
  const width = Math.max(span1, span2, span3, span4, span5);
  if (!Number.isFinite(width) || width <= EPS) return null;
  const tightening = span4 <= span1 * 0.85 || span4 <= span2 * 0.85;

  /** Contracting: successive legs shrink (B<A, C<B, D<C, E<D) in price extent. Expanding: the reverse. */
  const chainContract =
    span1 > EPS &&
    span2 + EPS < span1 &&
    span3 + EPS < span2 &&
    span4 + EPS < span3 &&
    span5 + EPS < span4;
  const chainExpand =
    span1 > EPS &&
    span2 > span1 + EPS &&
    span3 > span2 + EPS &&
    span4 > span3 + EPS &&
    span5 > span4 + EPS;

  const hSlope = highs[highs.length - 1].price - highs[0].price;
  const lSlope = lows[lows.length - 1].price - lows[0].price;
  const converging = hSlope <= EPS && lSlope >= -EPS;

  const envelope0 = Math.abs(highs[0].price - lows[0].price);
  const envelopeN = Math.abs(highs[highs.length - 1].price - lows[lows.length - 1].price);
  const envelopeContract = envelopeN <= envelope0 * 0.9;
  const envelopeExpand = envelopeN >= envelope0 * 1.05;

  const legA = Math.abs(pts[1].price - pts[0].price);
  const legB = Math.abs(pts[2].price - pts[1].price);
  const bVsA = legA > EPS ? legB / legA : 0;
  const triR5 = bVsA >= 0.382 && bVsA <= 1.618;

  const triR2 = triangleTriR2ChannelOk(pts);
  const triR3 = triangleTriR3ApexAfterE(pts);
  const triR4 = triangleTriR4NotParallel(pts);
  const triR7Exp = triangleTriR7ExpandingShortestAb(pts);

  const kindContract = converging && envelopeContract;
  const kindExpand = envelopeExpand && !envelopeContract && triR7Exp;

  const triR2e = (() => {
    const t = pts[5].index;
    const vAC = linearPriceAtIndex(t, pts[1].index, pts[1].price, pts[3].index, pts[3].price);
    const vBD = linearPriceAtIndex(t, pts[2].index, pts[2].price, pts[4].index, pts[4].price);
    const lo = Math.min(vAC, vBD);
    const hi = Math.max(vAC, vBD);
    const range = hi - lo;
    if (range <= EPS) return true;
    const d = Math.min(Math.abs(pts[5].price - lo), Math.abs(pts[5].price - hi));
    return d / range <= 0.15 + 1e-9;
  })();

  const checks: ElliottRuleCheckV2[] = [
    { id: "triangle_alt", passed: alternating },
    {
      id: "triangle_tightening",
      passed: tightening,
      detail: `${span4.toFixed(3)}<=${Math.max(span1, span2).toFixed(3)}`,
    },
    { id: "triangle_converging", passed: converging, detail: `hSlope=${hSlope.toFixed(3)} lSlope=${lSlope.toFixed(3)}` },
    { id: "triangle_envelope_contract", passed: envelopeContract, detail: `${envelopeN.toFixed(3)}<=${(envelope0 * 0.9).toFixed(3)}` },
    { id: "triangle_expanding", passed: envelopeExpand, detail: `zarf↑ ${envelopeN.toFixed(3)}≥${(envelope0 * 1.05).toFixed(3)}` },
    { id: "tri_r5", passed: triR5, detail: `B/A=${bVsA.toFixed(3)} ∈ [0.382,1.618]` },
    { id: "tri_r2_channel", passed: triR2, detail: "A–C / B–D şeridi" },
    { id: "tri_r2_e_deviation_15", passed: triR2e, detail: "E bant sapması ≤%15" },
    { id: "tri_r3_apex_after_e", passed: triR3, detail: "kesişim E sonrası" },
    { id: "tri_r4_not_parallel", passed: triR4, detail: "AC ∦ BD" },
    { id: "tri_r7_expanding_shortest_ab", passed: !envelopeExpand || triR7Exp, detail: envelopeExpand ? "genişleyen: en kısa A veya B" : "daralan: tri_r7 uygulanmaz" },
    {
      id: "tri_r1_substructure_note",
      passed: true,
      detail: "A–E iç yapı: `engine.ts` mikro zigzag ile `tri_*_corrective3` (teyit için bakın)",
    },
    {
      id: "triangle_context_wave2_wave4_post",
      passed: true,
      detail: context,
    },
    {
      id: "triangle_chain_contract",
      passed: !kindContract || chainContract,
      detail: `A..E spans: ${span1.toFixed(3)} ${span2.toFixed(3)} ${span3.toFixed(3)} ${span4.toFixed(3)} ${span5.toFixed(3)}`,
    },
    {
      id: "triangle_chain_expand",
      passed: !kindExpand || chainExpand,
      detail: `expand spans monotone: ${chainExpand}`,
    },
  ];
  const score = checks.filter((x) => x.passed).length + 0.2;

  if (!triR2 || !triR3 || !triR4 || !triR5) return null;
  if (kindContract && !chainContract) return null;
  if (kindExpand && !chainExpand) return null;
  if (kindContract) {
    return {
      pivots: [pts[0], pts[1], pts[2], pts[5]],
      path: pts,
      labels: ["a", "b", "c", "d", "e"],
      pattern: "triangle",
      checks,
      score,
    };
  }
  if (kindExpand && triR7Exp) {
    return {
      pivots: [pts[0], pts[1], pts[2], pts[5]],
      path: pts,
      labels: ["a", "b", "c", "d", "e"],
      pattern: "triangle",
      checks,
      score,
    };
  }
  return null;
}

/**
 * Motive bacaklarında (örn. diyagonal alt-dalga kanıtı): mikro zigzag ile yalnız ABC düzeltme araması.
 * `context` tabanlı triangle/combination kapıları uygulanmaz.
 */
export function detectNestedAbcCorrectiveInLeg(
  start: ZigzagPivot,
  end: ZigzagPivot,
  direction: "down" | "up",
  ohlc: OhlcV2[] | undefined,
  zigzag: ZigzagParams | undefined,
): CorrectiveCountV2 | null {
  if (!ohlc?.length || !zigzag) return null;
  const lo = Math.min(start.index, end.index);
  const hi = Math.max(start.index, end.index);
  const sub = ohlc.slice(lo, hi + 1);
  if (sub.length < 7) return null;

  const mainDepth = Math.max(2, Math.floor(zigzag.depth || 0));
  for (const depth of microDepthCandidatesForNestedLeg(mainDepth)) {
    if (sub.length < depth * 2 + 1) continue;
    const microLocal = buildZigzagPivotsV2(sub, { ...zigzag, depth });
    if (microLocal.length < 4) continue;
    const micro = microLocal.map((x) => ({ ...x, index: lo + x.index }));
    const inner = micro.filter((p) => p.index > start.index && p.index < end.index);
    const collect = direction === "down" ? collectAbcCorrectiveDown : collectAbcCorrectiveUp;
    const hit = collect(start, end, inner);
    if (hit) return hit;
  }
  return null;
}

/**
 * Y bölümünde (W–X’ten sonra) ardışık, çakışmayan kaç “zigzag” ABC’si çıkarılabilir.
 * Kombinasyonda tez olarak en fazla bir zigzag segmenti beklenir (§2.5.4.4).
 */
function countDisjointZigzagsInY(
  ySeq: ZigzagPivot[],
  direction: "down" | "up",
): number {
  if (ySeq.length < 4) return 0;
  const collect = direction === "down" ? collectAbcCorrectiveDown : collectAbcCorrectiveUp;
  let count = 0;
  let pos = 0;
  while (pos < ySeq.length - 3) {
    let nextEnd: number | null = null;
    for (let j = pos + 3; j < ySeq.length; j++) {
      const start = ySeq[pos];
      const end = ySeq[j];
      const inner = ySeq.slice(pos + 1, j);
      if (inner.length < 2) continue;
      const c = collect(start, end, inner);
      if (c?.pattern === "zigzag") {
        nextEnd = j;
        break;
      }
    }
    if (nextEnd === null) {
      pos++;
      continue;
    }
    count++;
    if (count > 1) return count;
    pos = nextEnd + 1;
  }
  return count;
}

/**
 * Kombinasyon Y bölümünde: üçgen benzeri yapı yalnızca **son** segmentte olmalı (tez).
 * Y’nin ilk 6 ucu üçgen geometrisine uyuyor ama son 6 uç uyumuyorsa adayı reddet.
 */
function combinationTriangleLastInYSegmentOk(ySeq: ZigzagPivot[], context: CorrectiveContext): boolean {
  if (ySeq.length < 7) return true;
  const head = ySeq.slice(0, 6);
  const tail = ySeq.slice(-6);
  const triHead = findTriangleBetween(head[0]!, head[5]!, head.slice(1, 5), context);
  const triTail = findTriangleBetween(tail[0]!, tail[5]!, tail.slice(1, 5), context);
  if (triHead && !triTail) return false;
  return true;
}

/**
 * §2.5.4.4 W–X–Y: yapısal teyit (comb_r1–comb_r4). Y tarafında en az iki ek salınım
 * (iç pivot ≥ 5 → toplam ≥ 7 uç) ki son parça “üçgen benzeri” karmaşıklık filtrelenebilsin.
 */
function findCombinationBetween(
  start: ZigzagPivot,
  end: ZigzagPivot,
  inner: ZigzagPivot[],
  direction: "down" | "up",
  context: CorrectiveContext,
): CorrectiveCountV2 | null {
  if (inner.length < 5) return null;
  const seq = [start, ...inner, end];
  const alternating = seq.every((p, i) => (i === 0 ? true : p.kind !== seq[i - 1].kind));
  if (!alternating) return null;
  const first = seq[0].price;
  const lastP = seq[seq.length - 1].price;
  const progressed = direction === "down" ? lastP < first - EPS : lastP > first + EPS;
  if (!progressed) return null;

  const legW = Math.abs(seq[1].price - seq[0].price);
  const legX = Math.abs(seq[2].price - seq[1].price);
  const ySegs: number[] = [];
  for (let i = 3; i < seq.length; i++) {
    ySegs.push(Math.abs(seq[i].price - seq[i - 1].price));
  }
  const pathY = ySegs.reduce((a, b) => a + b, 0);
  const maxYLeg = ySegs.length ? Math.max(...ySegs) : 0;

  const xVsW = legW > EPS ? legX / legW : 0;
  /** comb_r1 — üç parça ölçülebilir; X/W makul bant (yapısal bütünlük, §2.5.4.4) */
  const combR1 =
    legW > EPS && legX > EPS && maxYLeg > EPS && xVsW >= 0.12 && xVsW <= 1.05;
  /** comb_r2 — X, W’yi pratikte domine etmez (çoğunlukla zigzag bağlantısı) */
  const combR2 = legX <= legW * 0.95 + EPS;
  /** comb_r3 — Y bölümü (yol toplamı) cüce değil; son parça sık üçgen benzeri */
  const combR3 = pathY >= legX * 0.35 - EPS;
  /** comb_r4 — almaşıklık/kontrast: W ile Y tarafının güçlü bacakları farklı karakter */
  const maxWY = Math.max(legW, maxYLeg);
  const contrast = maxWY > EPS ? Math.abs(legW - maxYLeg) / maxWY : 0;
  const combR4 = contrast >= 0.12 - EPS;

  const ySeq = seq.slice(2);
  const zigzagCountY = countDisjointZigzagsInY(ySeq, direction);
  /** comb_r8 — Y içinde en fazla bir zigzag (çakışmasız ABC zigzag sayımı) */
  const combR8 = zigzagCountY <= 1;
  /** comb_r9 — Y’de üçgen yalnızca sonda (ilk 6 üçgen + son 6 değilse red) */
  const combR9 = combinationTriangleLastInYSegmentOk(ySeq, context);

  if (!combR1 || !combR2 || !combR3 || !combR4 || !combR8 || !combR9) return null;

  const checks: ElliottRuleCheckV2[] = [
    { id: "comb_alt", passed: alternating },
    { id: "comb_progress", passed: progressed },
    { id: "comb_r1", passed: combR1, detail: `X/W=${xVsW.toFixed(3)}` },
    { id: "comb_r2", passed: combR2, detail: `X≤0.95·W` },
    { id: "comb_r3", passed: combR3, detail: `pathY/X=${legX > EPS ? (pathY / legX).toFixed(3) : "—"}` },
    { id: "comb_r4", passed: combR4, detail: `kontrast(W,maxY)=${contrast.toFixed(3)}` },
    {
      id: "comb_r8_y_zigzag_cap",
      passed: combR8,
      detail: `Y zigzag sayısı=${zigzagCountY} (≤1)`,
    },
    {
      id: "comb_r9_y_triangle_last",
      passed: combR9,
      detail: "Y: reject if first-6 triangle-like but last-6 not (triangle last in Y)",
    },
    {
      id: "comb_r5",
      passed: true,
      detail: "W≈Y / Z uzatma: wxyxz_* (W–X–Y–X–Z motoru)",
    },
    {
      id: "comb_r6",
      passed: true,
      detail: `bağlam=${context}; WXYXZ post-B: wxyxz_post_b_context`,
    },
  ];
  const combConfirmed = combR1 && combR2 && combR3 && combR4 && combR8 && combR9;
  checks.push({ id: "comb_confirmed", passed: combConfirmed, detail: "W–X–Y teyit" });
  const score = checks.filter((x) => x.passed).length * 0.1 + 0.28;

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

/**
 * Same pattern label (zigzag vs flat): keep the highest-scoring ABC candidate.
 */
export function dedupeAbcScenariosByPattern(candidates: CorrectiveCountV2[]): CorrectiveCountV2[] {
  const m = new Map<CorrectivePatternV2, CorrectiveCountV2>();
  for (const c of candidates) {
    if (c.pattern !== "zigzag" && c.pattern !== "flat") continue;
    const prev = m.get(c.pattern);
    if (!prev || c.score > prev.score) m.set(c.pattern, c);
  }
  return [...m.values()].sort((a, b) => b.score - a.score);
}

/**
 * All valid ABC counts between wave-5 and the latest pivot after P5 (same window as the engine).
 */
export function collectAllAbcPostImpulseCandidates(
  pivots: ZigzagPivot[],
  impulse: ImpulseCountV2,
  toggles?: ElliottPatternMenuToggles,
): CorrectiveCountV2[] {
  const p5 = impulse.pivots[5];
  const later = pivots.filter((p) => p.index > p5.index);
  if (!later.length) return [];
  const end = later[later.length - 1]!;
  const inner = pivots.filter((p) => p.index > p5.index && p.index < end.index);
  if (inner.length < 2) return [];
  const t = mergePatternToggles(toggles);
  const dir = impulse.direction === "bull" ? "down" : "up";
  const raw =
    dir === "down" ? collectAllAbcCorrectiveDown(p5, end, inner) : collectAllAbcCorrectiveUp(p5, end, inner);
  const filtered = raw.filter((c) => {
    if (c.pattern === "zigzag" && !patternMenuAllowsZigzagAbc(t)) return false;
    if (c.pattern === "flat" && !patternMenuAllowsFlatAbc(t)) return false;
    return true;
  });
  return dedupeAbcScenariosByPattern(filtered);
}

/**
 * Observed A and B fixed; enumerate valid C endpoints from pivots after B (best score per pattern).
 */
export function collectAbcCorrectivesWithFixedAB(
  impulse: ImpulseCountV2,
  pivotA: ZigzagPivot,
  pivotB: ZigzagPivot,
  allPivots: ZigzagPivot[],
  toggles?: ElliottPatternMenuToggles,
): CorrectiveCountV2[] {
  const start = impulse.pivots[5];
  if (pivotA.index <= start.index || pivotB.index <= pivotA.index) return [];
  const ends = allPivots.filter((p) => p.index > pivotB.index);
  const t = mergePatternToggles(toggles);
  const candidates: CorrectiveCountV2[] = [];
  const dir = impulse.direction === "bull" ? "down" : "up";
  for (const end of ends) {
    const hit =
      dir === "down"
        ? abcFromQuadrupleDown(start, pivotA, pivotB, end)
        : abcFromQuadrupleUp(start, pivotA, pivotB, end);
    if (!hit) continue;
    if (hit.pattern === "zigzag" && !patternMenuAllowsZigzagAbc(t)) continue;
    if (hit.pattern === "flat" && !patternMenuAllowsFlatAbc(t)) continue;
    candidates.push(hit);
  }
  return dedupeAbcScenariosByPattern(candidates);
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

function mergeLegEndpointsMicro(micro: ZigzagPivot[], a: ZigzagPivot, b: ZigzagPivot): ZigzagPivot[] {
  const byIdx = new Map<number, ZigzagPivot>();
  for (const p of micro) {
    byIdx.set(p.index, p);
  }
  byIdx.set(a.index, a);
  byIdx.set(b.index, b);
  return [...byIdx.values()].sort((x, y) => x.index - y.index);
}

/**
 * Düzeltme bacakları (dalga 2 / 4): p1–p2 veya p3–p4 üzerinde mikro zigzag ile aynı düzeltme kuralları.
 */
export function detectNestedCorrectiveInLeg(
  mainPivots: ZigzagPivot[],
  start: ZigzagPivot,
  end: ZigzagPivot,
  direction: "down" | "up",
  context: CorrectiveContext,
  toggles: ElliottPatternMenuToggles | undefined,
  ohlc: OhlcV2[] | undefined,
  zigzag: ZigzagParams | undefined,
): CorrectiveCountV2 | null {
  if (!ohlc?.length || !zigzag) return null;
  const lo = Math.min(start.index, end.index);
  const hi = Math.max(start.index, end.index);
  const sub = ohlc.slice(lo, hi + 1);
  if (sub.length < 7) return null;

  const mainDepth = Math.max(2, Math.floor(zigzag.depth || 0));
  for (const depth of microDepthCandidatesForNestedLeg(mainDepth)) {
    if (sub.length < depth * 2 + 1) continue;
    const microLocal = buildZigzagPivotsV2(sub, { ...zigzag, depth });
    if (microLocal.length < 4) continue;
    const micro = microLocal.map((x) => ({ ...x, index: lo + x.index }));
    const merged = mergeLegEndpointsMicro(micro, start, end);
    const hit = findCorrectiveBetween(merged, start, end, direction, context, toggles);
    if (hit) return hit;
  }
  return null;
}

