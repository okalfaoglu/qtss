import type { ElliottRuleCheckV2, ImpulseCountV2, OhlcV2, ZigzagParams, ZigzagPivot } from "./types";
import { buildZigzagPivotsV2 } from "./zigzag";

const EPS = 1e-10;

/** Tez §2.5.3.4 ld_r3 — ilerleyen diyagonal: 5. dalga uzunluğu ≥ 1.382 × 4. dalga (Fib. uzantı) */
const LD_R3_W5_VS_W4_MIN = 1.382;

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
  const len2 = p1.price - p2.price;
  const len3 = p3.price - p2.price;
  const len5 = p5.price - p4.price;
  const w2NotLongerThanW1 = len2 <= len1 + EPS;
  checks.push({
    id: "w2_not_longer_than_w1",
    passed: w2NotLongerThanW1,
    detail: `|2|=${len2.toFixed(4)} <= |1|=${len1.toFixed(4)}`,
  });
  const w3NotShortest = !(len3 < len1 - EPS && len3 < len5 - EPS);
  checks.push({
    id: "w3_not_shortest_135",
    passed: w3NotShortest,
    detail: `|1|=${len1.toFixed(4)} |3|=${len3.toFixed(4)} |5|=${len5.toFixed(4)}`,
  });
  checks.push({
    id: "w3_not_below_w1_end",
    passed: p3.price >= p1.price - EPS,
    detail: `P3>=P1 (${p3.price.toFixed(4)}>=${p1.price.toFixed(4)})`,
  });

  /** Standart itkı — dalga 4 (|P3−P4|) dalga 3’ten (|P3−P2|) uzun olmamalı */
  const len4 = p3.price - p4.price;
  const w4NotLongerThanW3 = len4 <= len3 + EPS;
  checks.push({
    id: "w4_not_longer_than_w3",
    passed: w4NotLongerThanW3,
    detail: `|4|=${len4.toFixed(4)} <= |3|=${len3.toFixed(4)}`,
  });

  const overlap = p4.price > p1.price + EPS;
  checks.push({ id: "w4_no_overlap_w1", passed: overlap, detail: `P4>${p1.price.toFixed(4)}` });

  const trendShape = p3.price > p1.price && p5.price >= p3.price - EPS;
  checks.push({ id: "trend_shape", passed: trendShape });

  const w5ExtendsW3 = p5.price > p3.price + EPS;
  checks.push({
    id: "extension_w5_vs_w3",
    passed: w5ExtendsW3,
    detail: w5ExtendsW3
      ? `P5>P3 (${p5.price.toFixed(4)}>${p3.price.toFixed(4)})`
      : `kısaltılmış beşinci olası (P5≤P3)`,
  });

  const hardFail = checks.some(
    (c) =>
      !c.passed &&
      (c.id === "structure" || c.id.startsWith("w2_") || c.id.startsWith("w3_") || c.id.startsWith("w4_")),
  );
  const score = checks.filter((c) => c.passed).length;
  return { checks, hardFail, score };
}

/**
 * Diyagonal itkı (§2.5.3.4): w4–w1 bindirme serbest; ek kurallar `ed_r4`, `w5_not_longest_135`, `ld_r3`.
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
  const len2 = p1.price - p2.price;
  const w2NotLongerThanW1 = len2 <= len1 + EPS;
  checks.push({
    id: "w2_not_longer_than_w1",
    passed: w2NotLongerThanW1,
    detail: `|2|=${len2.toFixed(4)} <= |1|=${len1.toFixed(4)}`,
  });

  const len3 = p3.price - p2.price;
  const len5 = p5.price - p4.price;
  const len4 = p3.price - p4.price;

  checks.push({
    id: "w3_not_below_w1_end",
    passed: p3.price >= p1.price - EPS,
    detail: `P3>=P1 (${p3.price.toFixed(4)}>=${p1.price.toFixed(4)})`,
  });

  /** ed_r4 — 3’ün fiyat alanı (dikey) 2’den büyük: |P3−P2| > |P1−P2| */
  const edR4 = len3 > len2 + EPS;
  checks.push({
    id: "ed_r4_w3_area_gt_w2",
    passed: edR4,
    detail: `|3|=${len3.toFixed(4)} > |2|=${len2.toFixed(4)}`,
  });

  const w3NotShortest = !(len3 < len1 - EPS && len3 < len5 - EPS);
  checks.push({
    id: "w3_not_shortest_135",
    passed: w3NotShortest,
    detail: `|1|=${len1.toFixed(4)} |3|=${len3.toFixed(4)} |5|=${len5.toFixed(4)}`,
  });

  /** ed_r4 — 5, 1 ve 3’e göre en uzun itki dalgası olamaz */
  const w5NotLongest = !(len5 > len1 + EPS && len5 > len3 + EPS);
  checks.push({
    id: "w5_not_longest_135",
    passed: w5NotLongest,
    detail: `|5|=${len5.toFixed(4)} 1/3’e göre en uzun değil`,
  });

  /** ld_r3 — ilerleyen diyagonal: |5| ≥ 1.382 × |4| */
  const ldR3 = len5 + EPS >= LD_R3_W5_VS_W4_MIN * len4;
  checks.push({
    id: "ld_r3_w5_ge_1382_w4",
    passed: ldR3,
    detail: `|5|=${len5.toFixed(4)} ≥ ${LD_R3_W5_VS_W4_MIN}×|4|=${(LD_R3_W5_VS_W4_MIN * len4).toFixed(4)}`,
  });

  checks.push({
    id: "w4_diagonal_mode",
    passed: true,
    detail: "w4, w1 bölgesine girebilir (standart bindirme yasağı uygulanmaz)",
  });

  const trendShape = p3.price > p1.price && p5.price >= p3.price - EPS;
  checks.push({ id: "trend_shape", passed: trendShape });

  const w5ExtendsW3 = p5.price > p3.price + EPS;
  checks.push({
    id: "extension_w5_vs_w3",
    passed: w5ExtendsW3,
    detail: w5ExtendsW3
      ? `P5>P3 (${p5.price.toFixed(4)}>${p3.price.toFixed(4)})`
      : `kısaltılmış beşinci olası (P5≤P3)`,
  });

  const hardFail = checks.some(
    (c) =>
      !c.passed &&
      (c.id === "structure" ||
        c.id.startsWith("w2_") ||
        c.id.startsWith("w3_") ||
        c.id.startsWith("w5_") ||
        c.id === "ed_r4_w3_area_gt_w2" ||
        c.id === "ld_r3_w5_ge_1382_w4"),
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

  /**
   * Dalga 1 aşağı |P0−P1|, dalga 2 yukarı |P2−P1| — standart itkı ile aynı. `w2_not_beyond` (P2<P0)
   * sağlandığında len2≤len1 cebirsel olarak çoğu zaman otomatik gelir; yine de açık kontrol + skor.
   */
  const len1 = p0.price - p1.price;
  const len2 = p2.price - p1.price;
  const len3 = p2.price - p3.price;
  const len5 = p4.price - p5.price;
  const w2NotLongerThanW1 = len2 <= len1 + EPS;
  checks.push({
    id: "w2_not_longer_than_w1",
    passed: w2NotLongerThanW1,
    detail: `|2|=${len2.toFixed(4)} <= |1|=${len1.toFixed(4)}`,
  });
  const w3NotShortest = !(len3 < len1 - EPS && len3 < len5 - EPS);
  checks.push({
    id: "w3_not_shortest_135",
    passed: w3NotShortest,
    detail: `|1|=${len1.toFixed(4)} |3|=${len3.toFixed(4)} |5|=${len5.toFixed(4)}`,
  });
  checks.push({
    id: "w3_not_above_w1_end",
    passed: p3.price <= p1.price + EPS,
    detail: `P3<=P1 (${p3.price.toFixed(4)}<=${p1.price.toFixed(4)})`,
  });

  const len4 = p4.price - p3.price;
  const w4NotLongerThanW3 = len4 <= len3 + EPS;
  checks.push({
    id: "w4_not_longer_than_w3",
    passed: w4NotLongerThanW3,
    detail: `|4|=${len4.toFixed(4)} <= |3|=${len3.toFixed(4)}`,
  });

  const overlap = p4.price < p1.price - EPS;
  checks.push({ id: "w4_no_overlap_w1", passed: overlap, detail: `P4<${p1.price.toFixed(4)}` });

  const trendShape = p3.price < p1.price && p5.price <= p3.price + EPS;
  checks.push({ id: "trend_shape", passed: trendShape });

  const w5ExtendsW3 = p5.price < p3.price - EPS;
  checks.push({
    id: "extension_w5_vs_w3",
    passed: w5ExtendsW3,
    detail: w5ExtendsW3
      ? `P5<P3 (${p5.price.toFixed(4)}<${p3.price.toFixed(4)})`
      : `kısaltılmış beşinci olası (P5≥P3)`,
  });

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
  const len2 = p2.price - p1.price;
  const w2NotLongerThanW1 = len2 <= len1 + EPS;
  checks.push({
    id: "w2_not_longer_than_w1",
    passed: w2NotLongerThanW1,
    detail: `|2|=${len2.toFixed(4)} <= |1|=${len1.toFixed(4)}`,
  });

  const len3 = p2.price - p3.price;
  const len5 = p4.price - p5.price;
  const len4 = p4.price - p3.price;

  checks.push({
    id: "w3_not_above_w1_end",
    passed: p3.price <= p1.price + EPS,
    detail: `P3<=P1 (${p3.price.toFixed(4)}<=${p1.price.toFixed(4)})`,
  });

  /** ed_r4 — |P2−P3| > |P2−P1| */
  const edR4 = len3 > len2 + EPS;
  checks.push({
    id: "ed_r4_w3_area_gt_w2",
    passed: edR4,
    detail: `|3|=${len3.toFixed(4)} > |2|=${len2.toFixed(4)}`,
  });

  const w3NotShortest = !(len3 < len1 - EPS && len3 < len5 - EPS);
  checks.push({
    id: "w3_not_shortest_135",
    passed: w3NotShortest,
    detail: `|1|=${len1.toFixed(4)} |3|=${len3.toFixed(4)} |5|=${len5.toFixed(4)}`,
  });

  const w5NotLongest = !(len5 > len1 + EPS && len5 > len3 + EPS);
  checks.push({
    id: "w5_not_longest_135",
    passed: w5NotLongest,
    detail: `|5|=${len5.toFixed(4)} 1/3’e göre en uzun değil`,
  });

  const ldR3 = len5 + EPS >= LD_R3_W5_VS_W4_MIN * len4;
  checks.push({
    id: "ld_r3_w5_ge_1382_w4",
    passed: ldR3,
    detail: `|5|=${len5.toFixed(4)} ≥ ${LD_R3_W5_VS_W4_MIN}×|4|=${(LD_R3_W5_VS_W4_MIN * len4).toFixed(4)}`,
  });

  checks.push({
    id: "w4_diagonal_mode",
    passed: true,
    detail: "w4, w1 bölgesine girebilir (standart bindirme yasağı uygulanmaz)",
  });

  const trendShape = p3.price < p1.price && p5.price <= p3.price + EPS;
  checks.push({ id: "trend_shape", passed: trendShape });

  const w5ExtendsW3 = p5.price < p3.price - EPS;
  checks.push({
    id: "extension_w5_vs_w3",
    passed: w5ExtendsW3,
    detail: w5ExtendsW3
      ? `P5<P3 (${p5.price.toFixed(4)}<${p3.price.toFixed(4)})`
      : `kısaltılmış beşinci olası (P5≥P3)`,
  });

  const hardFail = checks.some(
    (c) =>
      !c.passed &&
      (c.id === "structure" ||
        c.id.startsWith("w2_") ||
        c.id.startsWith("w3_") ||
        c.id.startsWith("w5_") ||
        c.id === "ed_r4_w3_area_gt_w2" ||
        c.id === "ld_r3_w5_ge_1382_w4"),
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

function bestImpulseForSixPivots(
  win: [
    ZigzagPivot,
    ZigzagPivot,
    ZigzagPivot,
    ZigzagPivot,
    ZigzagPivot,
    ZigzagPivot,
  ],
  bull: boolean,
  opts?: ImpulseDetectOptions,
): ImpulseCountV2 | null {
  const allowStandard = opts?.allowStandard !== false;
  const allowDiagonal = opts?.allowDiagonal !== false;
  if (!allowStandard && !allowDiagonal) return null;
  let best: ImpulseCountV2 | null = null;
  if (allowStandard) {
    const r = bull ? checksBull(win) : checksBear(win);
    if (!r.hardFail) {
      const c: ImpulseCountV2 = {
        direction: bull ? "bull" : "bear",
        pivots: win,
        checks: r.checks,
        score: r.score,
        variant: "standard",
      };
      if (beatsImpulseCandidate(best, c)) best = c;
    }
  }
  if (allowDiagonal) {
    const r = bull ? checksBullDiagonal(win) : checksBearDiagonal(win);
    if (!r.hardFail) {
      const c: ImpulseCountV2 = {
        direction: bull ? "bull" : "bear",
        pivots: win,
        checks: r.checks,
        score: r.score,
        variant: "diagonal",
      };
      if (beatsImpulseCandidate(best, c)) best = c;
    }
  }
  return best;
}

/** Mikro pivotlar + bacak uçları (aynı bar indeksinde ana zigzag uçlarını zorunlu tutar). */
export function mergeLegEndpointsWithMicro(micro: ZigzagPivot[], p0: ZigzagPivot, p1: ZigzagPivot): ZigzagPivot[] {
  const byIdx = new Map<number, ZigzagPivot>();
  for (const p of micro) {
    byIdx.set(p.index, p);
  }
  byIdx.set(p0.index, p0);
  byIdx.set(p1.index, p1);
  return [...byIdx.values()].sort((a, b) => a.index - b.index);
}

/**
 * Zincirde ardışık 6 pivotluk tek pencere: tam olarak p0 (w0) ile p1 (w5) — iç itkı dalga 1 ile aynı mumlarda biter.
 */
function nestedImpulseExactEndpointsInMerged(
  merged: ZigzagPivot[],
  pa: ZigzagPivot,
  pb: ZigzagPivot,
  bull: boolean,
  opts?: ImpulseDetectOptions,
): ImpulseCountV2 | null {
  const i = merged.findIndex((p) => p.index === pa.index);
  if (i < 0 || merged.length < i + 6) return null;
  if (merged[i + 5]!.index !== pb.index) return null;
  const win = merged.slice(i, i + 6) as [
    ZigzagPivot,
    ZigzagPivot,
    ZigzagPivot,
    ZigzagPivot,
    ZigzagPivot,
    ZigzagPivot,
  ];
  return bestImpulseForSixPivots(win, bull, opts);
}

function microDepthCandidatesForNestedLeg(mainDepth: number): number[] {
  const mainD = Math.max(2, Math.floor(mainDepth || 0));
  const candDepths: number[] = [];
  for (const div of [2, 3, 4, 5, 6]) {
    const d = Math.max(2, Math.floor(mainD / div));
    if (d < mainD) candDepths.push(d);
  }
  candDepths.push(Math.max(2, mainD - 4), Math.max(2, mainD - 8), 5, 4, 3, 2);
  return [...new Set(candDepths)]
    .filter((d) => d >= 2 && d < mainD)
    .sort((a, b) => a - b);
}

/**
 * İtkı bacakları (dalga 1 / 3 / 5): pa→pb arasında alt derece 5’li itkı.
 * Uçlar pa ve pb ile tam hizalı tek 6’lı zincir; mikro zigzag ile aynı kural.
 */
export function detectNestedImpulseInLeg(
  pivots: ZigzagPivot[],
  pa: ZigzagPivot,
  pb: ZigzagPivot,
  opts?: ImpulseDetectOptions,
  ohlc?: OhlcV2[],
  zigzag?: ZigzagParams,
): ImpulseCountV2 | null {
  const lo = Math.min(pa.index, pb.index);
  const hi = Math.max(pa.index, pb.index);
  const bull = pb.price > pa.price;

  const slice = pivots.filter((p) => p.index >= lo && p.index <= hi);
  if (slice.length >= 6) {
    for (let start = 0; start + 6 <= slice.length; start++) {
      const win = slice.slice(start, start + 6) as [
        ZigzagPivot,
        ZigzagPivot,
        ZigzagPivot,
        ZigzagPivot,
        ZigzagPivot,
        ZigzagPivot,
      ];
      if (win[0].index !== pa.index || win[5].index !== pb.index) continue;
      const hit = bestImpulseForSixPivots(win, bull, opts);
      if (hit) return hit;
    }
  }

  if (!ohlc?.length || !zigzag) return null;
  const sub = ohlc.slice(lo, hi + 1);
  const n = sub.length;
  if (n < 7) return null;

  const mainDepth = Math.max(2, Math.floor(zigzag.depth || 0));
  for (const depth of microDepthCandidatesForNestedLeg(mainDepth)) {
    if (n < depth * 2 + 1) continue;
    const microLocal = buildZigzagPivotsV2(sub, { ...zigzag, depth });
    if (microLocal.length < 4) continue;
    const micro = microLocal.map((x) => ({ ...x, index: lo + x.index }));
    const merged = mergeLegEndpointsWithMicro(micro, pa, pb);
    const hit = nestedImpulseExactEndpointsInMerged(merged, pa, pb, bull, opts);
    if (hit) return hit;
  }
  return null;
}

/** Dalga 1 (p0→p1) için {@link detectNestedImpulseInLeg} sarmalayıcısı. */
export function detectWave1NestedImpulse(
  pivots: ZigzagPivot[],
  mainImpulse: ImpulseCountV2,
  opts?: ImpulseDetectOptions,
  ohlc?: OhlcV2[],
  zigzag?: ZigzagParams,
): ImpulseCountV2 | null {
  const p0 = mainImpulse.pivots[0];
  const p1 = mainImpulse.pivots[1];
  if (!p0 || !p1) return null;
  return detectNestedImpulseInLeg(pivots, p0, p1, opts, ohlc, zigzag);
}
