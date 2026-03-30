import type { SeriesMarker, UTCTimestamp } from "lightweight-charts";
import { DEFAULT_ELLIOTT_PATTERN_MENU, type ElliottPatternMenuToggles } from "../elliottPatternMenuCatalog";
import type { PatternLayerOverlay } from "../patternDrawingBatchOverlay";
import type { CorrectiveCountV2, ElliottEngineOutputV2, ImpulseCountV2, OhlcV2, Timeframe } from "./types";

function impulseShownInMenu(imp: ImpulseCountV2, menu?: ElliottPatternMenuToggles): boolean {
  const m = { ...DEFAULT_ELLIOTT_PATTERN_MENU, ...menu };
  const v = imp.variant ?? "standard";
  if (v === "diagonal") return m.motive_diagonal;
  return m.motive_impulse;
}

type Pt = { time: UTCTimestamp; value: number };

export type ElliottProjectionV2Options = {
  barHop: number;
  maxSteps: number;
};

function markerLabel(step: number, tf: Timeframe): string {
  const seq =
    tf === "4h"
      ? ["Ⓐ", "Ⓑ", "Ⓒ", "①", "②", "③", "④", "⑤"]
      : tf === "1h"
        ? ["(A)", "(B)", "(C)", "(1)", "(2)", "(3)", "(4)", "(5)"]
        : ["a", "b", "c", "i", "ii", "iii", "iv", "v"];
  return seq[(step - 1) % seq.length] ?? String(step);
}

function chooseImpulse(out: ElliottEngineOutputV2) {
  return out.hierarchy.micro?.impulse ?? out.hierarchy.intermediate?.impulse ?? out.hierarchy.macro?.impulse ?? null;
}

function impulseForProjectionTf(out: ElliottEngineOutputV2, tf: Timeframe): ImpulseCountV2 | null {
  return out.states[tf]?.impulse ?? null;
}

function postAbcForProjectionTf(out: ElliottEngineOutputV2, tf: Timeframe): CorrectiveCountV2 | null {
  return out.states[tf]?.postImpulseAbc ?? null;
}

function inferBarStepSec(rows: OhlcV2[]): number {
  if (rows.length < 2) return 60;
  const tail = rows.slice(-120);
  let sum = 0;
  let n = 0;
  for (let i = 1; i < tail.length; i++) {
    const d = tail[i].t - tail[i - 1].t;
    if (d > 0) {
      sum += d;
      n++;
    }
  }
  return n ? Math.max(30, Math.round(sum / n)) : 60;
}

/**
 * 1-5 kabulünden sonra projeksiyonun "mevcut duruma" hizası:
 * - Son mum p5'e gore nerede? (duzeltme/asiri uzama)
 * - Sonraki adimi A/B/C veya 1/2/3/... olarak sec.
 */
function startStepFromCurrentState(isBull: boolean, p5: number, latest: number, base: number): number {
  const dir = isBull ? 1 : -1;
  const x = ((latest - p5) * dir) / Math.max(1e-8, base);

  // x < 0: itkiye ters yönde düzeltme (A/B/C). Üst sınır `>` ile değil `>=` ile: örn. x = -0.22 ile
  // x = -0.219999 aynı bantta kalsın (yarı-açık sola: A = [-0.22, 0), B = [-0.58, -0.22), C < -0.58).
  if (x < 0) {
    if (x >= -0.22) return 1; // A
    if (x >= -0.58) return 2; // B
    return 3; // C veya C sonu (yalnızca A/B teyidi varsa kullanılmalı)
  }

  // x >= 0: p5 üstü/altında trend yönünde; [0,0.25), [0.25,0.75), … yarı-açık bantlar.
  if (x < 0.25) return 4; // yeni 1
  if (x < 0.75) return 5; // 2
  if (x < 1.6) return 6; // 3
  if (x < 2.1) return 7; // 4
  return 8; // 5
}

function stepDirection(step: number): number {
  // A/C/2/4 ters yon, B/1/3/5 trend yonu
  const inCycle = ((step - 1) % 8) + 1;
  return inCycle === 1 || inCycle === 3 || inCycle === 5 || inCycle === 7 ? -1 : 1;
}

type ProjectionCalibration = {
  aMul: number;
  bMul: number;
  cMul: number;
  i1Mul: number;
  i2Mul: number;
  i3Mul: number;
  i4Mul: number;
  i5Mul: number;
};

const DEFAULT_CALIBRATION: ProjectionCalibration = {
  aMul: 0.382,
  bMul: 0.236,
  cMul: 0.618,
  i1Mul: 1.0,
  i2Mul: 0.382,
  i3Mul: 1.618,
  i4Mul: 0.382,
  i5Mul: 1.0,
};

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

function median(nums: number[]): number {
  if (!nums.length) return 0;
  const s = [...nums].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  if (s.length % 2) return s[m]!;
  return (s[m - 1]! + s[m]!) / 2;
}

/** Son mumların fiyat/s hızını itkı ortalama hızıyla kıyasla; piyasa hızlandıkça projeksiyon süreleri kısalır. */
function recentVsImpulseVelocityMul(rows: OhlcV2[], refRatePerSec: number): number {
  const tail = rows.slice(-40);
  if (tail.length < 4 || refRatePerSec <= 1e-12) return 1;
  let sumDp = 0;
  let sumDt = 0;
  for (let i = 1; i < tail.length; i++) {
    const dt = tail[i]!.t - tail[i - 1]!.t;
    if (dt <= 0) continue;
    sumDp += Math.abs(tail[i]!.c - tail[i - 1]!.c);
    sumDt += dt;
  }
  const recent = sumDt > 0 ? sumDp / sumDt : refRatePerSec;
  return clamp(recent / refRatePerSec, 0.48, 2.05);
}

/** Projeksiyon başlangıcından bu segment uca kadar yaklaşık süre (işaret metni). */
function formatEtaFromStart(deltaSec: number): string {
  const s = Math.max(0, Math.round(deltaSec));
  if (s < 3600) return `+${Math.max(1, Math.round(s / 60))}m`;
  if (s < 172800) return `+${Math.round(s / 3600)}h`;
  return `+${Math.round(s / 86400)}d`;
}

/**
 * Post-ABC tespitinden dinamik oran profili uret:
 * - A/B/C carpani gercek segment buyukluklerinden gelir.
 * - Impuls 1-5 carpani C ve onceki ortalamaya gore hafif ayarlanir.
 */
function buildCalibrationFromPostAbc(
  post: CorrectiveCountV2 | null,
  base: number,
): ProjectionCalibration {
  if (!post) return DEFAULT_CALIBRATION;
  const path = post.path?.length ? post.path : post.pivots;
  if (path.length < 4) return DEFAULT_CALIBRATION;

  const a = Math.abs(path[1]!.price - path[0]!.price);
  const b = Math.abs(path[2]!.price - path[1]!.price);
  const c = Math.abs(path[3]!.price - path[2]!.price);
  const d = Math.max(1e-8, base);

  const aMul = clamp(a / d, 0.18, 1.2);
  const bMul = clamp(b / d, 0.12, 0.9);
  const cMul = clamp(c / d, 0.28, 2.2);

  // C ne kadar gucluyse yeni impuls 1/3 bir miktar buyusun.
  const cBoost = clamp(cMul / Math.max(1e-8, aMul), 0.75, 1.45);
  const i1Mul = clamp(0.85 * cBoost, 0.55, 1.35);
  const i2Mul = clamp(0.35 * (bMul / Math.max(1e-8, aMul)), 0.22, 0.62);
  const i3Mul = clamp(1.35 * cBoost, 1.0, 2.35);
  const i4Mul = clamp(i2Mul, 0.22, 0.62);
  const i5Mul = clamp(0.9 * cBoost, 0.6, 1.55);

  return { aMul, bMul, cMul, i1Mul, i2Mul, i3Mul, i4Mul, i5Mul };
}

function stepMagnitudeWithCalibration(step: number, base: number, cal: ProjectionCalibration): number {
  const inCycle = ((step - 1) % 8) + 1;
  switch (inCycle) {
    case 1:
      return base * cal.aMul; // A
    case 2:
      return base * cal.bMul; // B
    case 3:
      return base * cal.cMul; // C
    case 4:
      return base * cal.i1Mul; // 1
    case 5:
      return base * cal.i2Mul; // 2
    case 6:
      return base * cal.i3Mul; // 3
    case 7:
      return base * cal.i4Mul; // 4
    default:
      return base * cal.i5Mul; // 5
  }
}

type RateProfile = {
  motiveRate: number; // price/sec
  corrRate: number; // price/sec
};

function buildRateProfile(imp: ImpulseCountV2, post: CorrectiveCountV2 | null): RateProfile {
  const motiveRates: number[] = [];
  const corrRates: number[] = [];
  const p = imp.pivots;
  const pushPairs: Array<[number, number]> = [
    [0, 1], // wave1
    [2, 3], // wave3
    [4, 5], // wave5
  ];
  const corrPairs: Array<[number, number]> = [
    [1, 2], // wave2
    [3, 4], // wave4
  ];

  for (const [a, b] of pushPairs) {
    const dt = Math.max(1, p[b]!.time - p[a]!.time);
    const dp = Math.abs(p[b]!.price - p[a]!.price);
    motiveRates.push(dp / dt);
  }
  for (const [a, b] of corrPairs) {
    const dt = Math.max(1, p[b]!.time - p[a]!.time);
    const dp = Math.abs(p[b]!.price - p[a]!.price);
    corrRates.push(dp / dt);
  }

  if (post) {
    const path = post.path?.length ? post.path : post.pivots;
    for (let i = 1; i < path.length; i++) {
      const dt = Math.max(1, path[i]!.time - path[i - 1]!.time);
      const dp = Math.abs(path[i]!.price - path[i - 1]!.price);
      corrRates.push(dp / dt);
    }
  }

  const motiveRate = Math.max(1e-8, median(motiveRates));
  const corrRate = Math.max(1e-8, corrRates.length ? median(corrRates) : motiveRate * 0.62);
  return { motiveRate, corrRate };
}

function stepIsCorrective(step: number): boolean {
  const inCycle = ((step - 1) % 8) + 1;
  return inCycle <= 3;
}

function startStepFromPostAbc(
  isBull: boolean,
  post: CorrectiveCountV2 | null,
  latest: number,
  base: number,
): number | null {
  if (!post) return null;
  const path = post.path?.length ? post.path : post.pivots;
  if (!path.length) return null;
  const end = path[path.length - 1]!;
  const d = ((latest - end.price) * (isBull ? 1 : -1)) / Math.max(1e-8, base);

  // post-ABC son pivota gore:
  // d<0: C devam ediyor/uzuyor -> C fazi
  // d~0: C tamamlandi -> yeni 1
  // d>0: yeni impuls ilerliyor -> 1/2/3...
  if (d < -0.18) return 3; // C
  if (d < 0.22) return 4; // yeni 1
  if (d < 0.68) return 5; // 2
  if (d < 1.55) return 6; // 3
  if (d < 2.05) return 7; // 4
  return 8; // 5
}

/**
 * Lightweight V2 forward projection:
 * - `sourceTf` doluysa o TF itkisi kullanılır; çizim çapası **aynı TF’in** `ohlcByTf` son mumu olmalı
 *   (ana grafik farklı intervaldeyse `anchorRows` son mumu itkı ile yanlış hizalanırdı).
 * - Segment süresi: `Δt ≈ |Δfiyat| / hız` (itkı/düzeltme için ölçülen pivot hızları + güncel mum volatilitesi).
 * - `barHop`: bir segment için nominal süre ölçeği (`≈ hop × ortalama mum aralığı`); gerçek `Δt` bant içinde kalır.
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
  const barPeriodSec = inferBarStepSec(rowsForStep);
  const hop = Math.max(1, Math.floor(opt.barHop || 1));
  const stepSec = barPeriodSec * hop;
  const maxSteps = Math.min(24, Math.max(1, Math.floor(opt.maxSteps || 12)));

  const len1 = Math.abs(p[1].price - p[0].price);
  const len3 = Math.abs(p[3].price - p[2].price);
  const len5 = Math.abs(p[5].price - p[4].price);
  const base = Math.max(1e-8, (len1 + len3 + len5) / 3);

  const anchorLast = rowsForStep.length ? rowsForStep[rowsForStep.length - 1]! : anchorRows[anchorRows.length - 1]!;
  const startPrice = anchorLast.c;
  const startTime = anchorLast.t;
  const startStepRaw =
    startStepFromPostAbc(isBull, postAbc, startPrice, base) ??
    startStepFromCurrentState(isBull, p5.price, startPrice, base);
  const postPath = postAbc ? (postAbc.path?.length ? postAbc.path : postAbc.pivots) : null;
  const hasObservedAB = !!postPath && postPath.length >= 3;
  // A/B teyidi yoksa C'den (veya B'den) başlatma: düzeltme her zaman A'dan başlar.
  const startStep = !hasObservedAB && startStepRaw < 4 ? 1 : startStepRaw;
  const cal = buildCalibrationFromPostAbc(postAbc, base);
  const rates = buildRateProfile(imp, postAbc);
  const refBlendRate = (rates.motiveRate + rates.corrRate) * 0.5;
  const regimeMul = recentVsImpulseVelocityMul(rowsForStep, refBlendRate);

  const points: Pt[] = [{ time: startTime as UTCTimestamp, value: startPrice }];
  const markers: SeriesMarker<UTCTimestamp>[] = [];
  const layers: PatternLayerOverlay[] = [];

  let cur = startPrice;
  let t = startTime as number;
  const markerColor =
    lineColor && /^#[0-9A-Fa-f]{3,8}$/.test(lineColor.trim()) ? lineColor.trim() : "#64b5f6";

  for (let i = 0; i < maxSteps; i++) {
    const stepNo = startStep + i;
    const magRaw = stepMagnitudeWithCalibration(stepNo, base, cal);
    const stepRateBase = stepIsCorrective(stepNo) ? rates.corrRate : rates.motiveRate;
    const stepRate = Math.max(1e-12, stepRateBase * regimeMul);
    const magNominal = stepRate * stepSec;
    const mag = clamp(magRaw, magNominal * 0.42, magNominal * 1.88);
    const dir = stepDirection(stepNo);
    const signed = (isBull ? 1 : -1) * dir * mag;
    cur += signed;
    const deltaPrice = Math.abs(signed);
    const dtRaw = deltaPrice / stepRate;
    // At least one full bar period per leg so the polyline is readable on MTF charts:
    // sub-bar minDt (e.g. 0.28×15m ≈ 4.2m) collapses all steps into one vertical pixel column
    // when the visible history spans hundreds of candles (looks like a thick vertical spike).
    const minDt = Math.max(45, Math.round(stepSec * 0.28), barPeriodSec);
    const maxDt = Math.max(minDt + 1, Math.round(stepSec * 7.5));
    t += Math.round(clamp(dtRaw, minDt, maxDt));
    points.push({ time: t as UTCTimestamp, value: cur });

    markers.push({
      time: t as UTCTimestamp,
      position: signed >= 0 ? "aboveBar" : "belowBar",
      shape: "circle",
      color: markerColor,
      text: `${markerLabel(stepNo, tf)} ${formatEtaFromStart(t - startTime)}`,
    });
  }

  // 1-2-3-4-5 sonrasinda ABC teyit aramasi:
  // A ve B varsa bu kisimlar duz (done), C tamamlanmadiysa B->son fiyat kesik gosterilir.
  if (postPath && postPath.length >= 3) {
    const a = postPath[1]!;
    const b = postPath[2]!;
    const c = postPath.length >= 4 ? postPath[3]! : null;
    const dir = isBull ? 1 : -1;
    const cCompleted = !!c && startPrice * dir > c.price * dir + 0.05 * base;
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
    if (c && !cCompleted) {
      layers.push({
        upper: [],
        lower: [],
        zigzag: [
          { time: b.time as UTCTimestamp, value: b.price },
          { time: startTime as UTCTimestamp, value: startPrice },
        ],
        zigzagKind: "elliott_projection_c_active",
        zigzagLineColor: lineColor,
      });
    }
  }

  layers.push({
    upper: [],
    lower: [],
    zigzag: points,
    zigzagKind: "elliott_projection",
    zigzagLineColor: lineColor,
    zigzagMarkers: markers,
  });

  return {
    layers,
  };
}

