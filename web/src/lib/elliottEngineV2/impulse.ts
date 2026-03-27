import type { ElliottRuleCheckV2, ImpulseCountV2, ZigzagPivot } from "./types";

const EPS = 1e-10;

export type ImpulseDetectOptions = {
  allowStandard?: boolean;
  allowDiagonal?: boolean;
};

function checksBull(
  p: [ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot],
): { checks: ElliottRuleCheckV2[]; hardFail: boolean; score: number } {
  const [p0, p1, p2, p3, p4, p5] = p;
  const checks: ElliottRuleCheckV2[] = [];

  const struct =
    p0.kind === "low" &&
    p1.kind === "high" &&
    p2.kind === "low" &&
    p3.kind === "high" &&
    p4.kind === "low" &&
    p5.kind === "high";
  checks.push({ id: "structure", passed: struct });

  const w2 = p2.price > p0.price + EPS;
  checks.push({ id: "w2_not_beyond_w1_start", passed: w2, detail: `P2>${p0.price.toFixed(4)}` });

  const len1 = p1.price - p0.price;
  const len3 = p3.price - p2.price;
  const len5 = p5.price - p4.price;
  const w3NotShortest = !(len3 < len1 - EPS && len3 < len5 - EPS);
  checks.push({
    id: "w3_not_shortest_135",
    passed: w3NotShortest,
    detail: `|1|=${len1.toFixed(4)} |3|=${len3.toFixed(4)} |5|=${len5.toFixed(4)}`,
  });

  const overlap = p4.price > p1.price + EPS;
  checks.push({ id: "w4_no_overlap_w1", passed: overlap, detail: `P4>${p1.price.toFixed(4)}` });

  const trendShape = p3.price > p1.price && p5.price >= p3.price - EPS;
  checks.push({ id: "trend_shape", passed: trendShape });

  const hardFail = checks.some(
    (c) =>
      !c.passed &&
      (c.id === "structure" || c.id.startsWith("w2_") || c.id.startsWith("w3_") || c.id.startsWith("w4_")),
  );
  const score = checks.filter((c) => c.passed).length;
  return { checks, hardFail, score };
}

/**
 * Sonlanan / ilerleyen diyagonal: w2/w3/omurga aynı; w4, w1 fiyat alanına girebilir — katı w4–w1
 * ayrım kontrolü yok (elliottPatternMenu: Diagonals).
 */
function checksBullDiagonal(
  p: [ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot],
): { checks: ElliottRuleCheckV2[]; hardFail: boolean; score: number } {
  const [p0, p1, p2, p3, p4, p5] = p;
  const checks: ElliottRuleCheckV2[] = [];

  const struct =
    p0.kind === "low" &&
    p1.kind === "high" &&
    p2.kind === "low" &&
    p3.kind === "high" &&
    p4.kind === "low" &&
    p5.kind === "high";
  checks.push({ id: "structure", passed: struct });

  const w2 = p2.price > p0.price + EPS;
  checks.push({ id: "w2_not_beyond_w1_start", passed: w2, detail: `P2>${p0.price.toFixed(4)}` });

  const len1 = p1.price - p0.price;
  const len3 = p3.price - p2.price;
  const len5 = p5.price - p4.price;
  const w3NotShortest = !(len3 < len1 - EPS && len3 < len5 - EPS);
  checks.push({
    id: "w3_not_shortest_135",
    passed: w3NotShortest,
    detail: `|1|=${len1.toFixed(4)} |3|=${len3.toFixed(4)} |5|=${len5.toFixed(4)}`,
  });

  checks.push({
    id: "w4_diagonal_mode",
    passed: true,
    detail: "w4, w1 bölgesine girebilir (standart bindirme yasağı uygulanmaz)",
  });

  const trendShape = p3.price > p1.price && p5.price >= p3.price - EPS;
  checks.push({ id: "trend_shape", passed: trendShape });

  const hardFail = checks.some(
    (c) =>
      !c.passed &&
      (c.id === "structure" || c.id.startsWith("w2_") || c.id.startsWith("w3_")),
  );
  const score = checks.filter((c) => c.passed).length;
  return { checks, hardFail, score };
}

function checksBear(
  p: [ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot],
): { checks: ElliottRuleCheckV2[]; hardFail: boolean; score: number } {
  const [p0, p1, p2, p3, p4, p5] = p;
  const checks: ElliottRuleCheckV2[] = [];

  const struct =
    p0.kind === "high" &&
    p1.kind === "low" &&
    p2.kind === "high" &&
    p3.kind === "low" &&
    p4.kind === "high" &&
    p5.kind === "low";
  checks.push({ id: "structure", passed: struct });

  const w2 = p2.price < p0.price - EPS;
  checks.push({ id: "w2_not_beyond_w1_start", passed: w2, detail: `P2<${p0.price.toFixed(4)}` });

  const len1 = p0.price - p1.price;
  const len3 = p2.price - p3.price;
  const len5 = p4.price - p5.price;
  const w3NotShortest = !(len3 < len1 - EPS && len3 < len5 - EPS);
  checks.push({
    id: "w3_not_shortest_135",
    passed: w3NotShortest,
    detail: `|1|=${len1.toFixed(4)} |3|=${len3.toFixed(4)} |5|=${len5.toFixed(4)}`,
  });

  const overlap = p4.price < p1.price - EPS;
  checks.push({ id: "w4_no_overlap_w1", passed: overlap, detail: `P4<${p1.price.toFixed(4)}` });

  const trendShape = p3.price < p1.price && p5.price <= p3.price + EPS;
  checks.push({ id: "trend_shape", passed: trendShape });

  const hardFail = checks.some(
    (c) =>
      !c.passed &&
      (c.id === "structure" || c.id.startsWith("w2_") || c.id.startsWith("w3_") || c.id.startsWith("w4_")),
  );
  const score = checks.filter((c) => c.passed).length;
  return { checks, hardFail, score };
}

function checksBearDiagonal(
  p: [ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot, ZigzagPivot],
): { checks: ElliottRuleCheckV2[]; hardFail: boolean; score: number } {
  const [p0, p1, p2, p3, p4, p5] = p;
  const checks: ElliottRuleCheckV2[] = [];

  const struct =
    p0.kind === "high" &&
    p1.kind === "low" &&
    p2.kind === "high" &&
    p3.kind === "low" &&
    p4.kind === "high" &&
    p5.kind === "low";
  checks.push({ id: "structure", passed: struct });

  const w2 = p2.price < p0.price - EPS;
  checks.push({ id: "w2_not_beyond_w1_start", passed: w2, detail: `P2<${p0.price.toFixed(4)}` });

  const len1 = p0.price - p1.price;
  const len3 = p2.price - p3.price;
  const len5 = p4.price - p5.price;
  const w3NotShortest = !(len3 < len1 - EPS && len3 < len5 - EPS);
  checks.push({
    id: "w3_not_shortest_135",
    passed: w3NotShortest,
    detail: `|1|=${len1.toFixed(4)} |3|=${len3.toFixed(4)} |5|=${len5.toFixed(4)}`,
  });

  checks.push({
    id: "w4_diagonal_mode",
    passed: true,
    detail: "w4, w1 bölgesine girebilir (standart bindirme yasağı uygulanmaz)",
  });

  const trendShape = p3.price < p1.price && p5.price <= p3.price + EPS;
  checks.push({ id: "trend_shape", passed: trendShape });

  const hardFail = checks.some(
    (c) =>
      !c.passed &&
      (c.id === "structure" || c.id.startsWith("w2_") || c.id.startsWith("w3_")),
  );
  const score = checks.filter((c) => c.passed).length;
  return { checks, hardFail, score };
}

function beatsImpulseCandidate(best: ImpulseCountV2 | null, cand: ImpulseCountV2): boolean {
  if (!best) return true;
  if (cand.score !== best.score) return cand.score > best.score;
  const bv = best.variant ?? "standard";
  const cv = cand.variant ?? "standard";
  if (cv === "standard" && bv === "diagonal") return true;
  return false;
}

/**
 * İt düzleminde en iyi aday: standart itki (w4–w1 bindirme yok) ve/veya diyagonal (bindirme serbest).
 * `pattern_menu` ile yalnızca açık olan türler değerlendirilir.
 */
export function detectBestImpulseV2(
  pivots: ZigzagPivot[],
  maxWindows = 80,
  opts?: ImpulseDetectOptions,
): ImpulseCountV2 | null {
  const allowStandard = opts?.allowStandard !== false;
  const allowDiagonal = opts?.allowDiagonal !== false;
  if (!allowStandard && !allowDiagonal) return null;

  if (pivots.length < 6) return null;
  let best: ImpulseCountV2 | null = null;

  for (let k = 0; k < maxWindows; k++) {
    const start = pivots.length - 6 - k;
    if (start < 0) break;
    const s = pivots.slice(start, start + 6) as [
      ZigzagPivot,
      ZigzagPivot,
      ZigzagPivot,
      ZigzagPivot,
      ZigzagPivot,
      ZigzagPivot,
    ];
    if (allowStandard) {
      const bull = checksBull(s);
      if (!bull.hardFail) {
        const c: ImpulseCountV2 = {
          direction: "bull",
          pivots: s,
          checks: bull.checks,
          score: bull.score,
          variant: "standard",
        };
        if (beatsImpulseCandidate(best, c)) best = c;
      }
      const bear = checksBear(s);
      if (!bear.hardFail) {
        const c: ImpulseCountV2 = {
          direction: "bear",
          pivots: s,
          checks: bear.checks,
          score: bear.score,
          variant: "standard",
        };
        if (beatsImpulseCandidate(best, c)) best = c;
      }
    }
    if (allowDiagonal) {
      const bull = checksBullDiagonal(s);
      if (!bull.hardFail) {
        const c: ImpulseCountV2 = {
          direction: "bull",
          pivots: s,
          checks: bull.checks,
          score: bull.score,
          variant: "diagonal",
        };
        if (beatsImpulseCandidate(best, c)) best = c;
      }
      const bear = checksBearDiagonal(s);
      if (!bear.hardFail) {
        const c: ImpulseCountV2 = {
          direction: "bear",
          pivots: s,
          checks: bear.checks,
          score: bear.score,
          variant: "diagonal",
        };
        if (beatsImpulseCandidate(best, c)) best = c;
      }
    }
  }

  return best;
}

/**
 * Geçmiş pencerelerdeki itki adaylarını döndürür (çakışanlar elenir).
 * Amaç: grafikte "tarihsel Elliott tarama" katmanı.
 */
export function detectHistoricalImpulsesV2(
  pivots: ZigzagPivot[],
  maxWindows = 240,
  maxCount = 16,
  opts?: ImpulseDetectOptions,
): ImpulseCountV2[] {
  if (pivots.length < 6 || maxCount < 1) return [];
  const out: ImpulseCountV2[] = [];
  const ranges: Array<{ start: number; end: number }> = [];

  // Eskiden yeniye tara; görselde kronolojik bütünlük sağlar.
  for (let k = maxWindows - 1; k >= 0; k--) {
    const start = pivots.length - 6 - k;
    if (start < 0) continue;
    const slice = pivots.slice(start, start + 6);
    if (slice.length < 6) continue;
    const cand = detectBestImpulseV2(slice, 1, opts);
    if (!cand) continue;
    const r = { start: cand.pivots[0].index, end: cand.pivots[5].index };
    const overlaps = ranges.some((x) => !(r.end < x.start || r.start > x.end));
    if (overlaps) continue;
    ranges.push(r);
    out.push(cand);
    if (out.length >= maxCount) break;
  }
  return out;
}
