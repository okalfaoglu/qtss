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
  /** Projection rendering mode. */
  mode?: "legacy" | "formation";
  /**
   * Fib time blend weight for measured impulse durations vs fixed defaults.
   * `0` => only fixed defaults, `1` => only measured ratios.
   * @default 0.5
   */
  fibMeasuredWeight?: number;
  /**
   * İkinci polyline: uzatılmış 3. dalga senaryosu. `false` veya çıkarılırsa yalnızca birincil yol.
   * @default true
   */
  includeAltScenario?: boolean;
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

/** Tespit edilen düzeltme kalıbına göre projeksiyon büyüklüklerine hafif düzeltme. */
function applyFormationCalibrationTweak(
  post: CorrectiveCountV2 | null,
  cal: ProjectionCalibration,
): ProjectionCalibration {
  if (!post) return cal;
  switch (post.pattern) {
    case "triangle":
      return {
        ...cal,
        cMul: clamp(cal.cMul * 0.9, 0.2, 2.2),
        i3Mul: clamp(cal.i3Mul * 0.94, 0.85, 2.45),
      };
    case "flat":
      return {
        ...cal,
        bMul: clamp(cal.bMul * 1.06, 0.12, 0.95),
        aMul: clamp(cal.aMul * 1.03, 0.18, 1.2),
      };
    case "combination":
      return {
        ...cal,
        cMul: clamp(cal.cMul * 1.05, 0.28, 2.3),
      };
    default:
      return cal;
  }
}

/** Alternatif senaryo: 3. dalga hedefi güçlü, 5. hafif kısılır. */
function extendedThirdWaveCalibration(cal: ProjectionCalibration): ProjectionCalibration {
  return {
    ...cal,
    i3Mul: clamp(cal.i3Mul * 1.2, 1.05, 2.55),
    i5Mul: clamp(cal.i5Mul * 0.92, 0.55, 1.55),
    i2Mul: clamp(cal.i2Mul * 0.95, 0.18, 0.65),
  };
}

/**
 * Elliott tipik süre oranları + ölçülen itkı bacak sürelerinin karışımı.
 * `dt` çarpanı olarak kullanılır (fiyat hızı ile çarpılmış ham süre üzerine).
 */
function projectionFibTimeMultiplier(
  inCycle: number,
  imp: ImpulseCountV2,
  post: CorrectiveCountV2 | null,
  fibMeasuredWeight: number,
): number {
  const p = imp.pivots;
  const d01 = Math.max(1, p[1].time - p[0].time);
  const d12 = Math.max(1, p[2].time - p[1].time);
  const d23 = Math.max(1, p[3].time - p[2].time);
  const d34 = Math.max(1, p[4].time - p[3].time);
  const d45 = Math.max(1, p[5].time - p[4].time);
  const w1 = Math.max(60, d01);

  const fibDefaults: Record<number, number> = {
    1: 1.0,
    2: 0.618,
    3: 1.618,
    4: 1.0,
    5: 0.5,
    6: 1.618,
    7: 0.382,
    8: 1.0,
  };
  const def = fibDefaults[inCycle] ?? 1;

  if (inCycle <= 3) {
    let corrRef = Math.max(60, (d12 + d34) / 2);
    const path = post?.path?.length ? post.path : post?.pivots;
    if (path && path.length >= 2) {
      const seg: number[] = [];
      for (let i = 1; i < path.length; i++) {
        seg.push(Math.max(1, path[i]!.time - path[i - 1]!.time));
      }
      if (seg.length) corrRef = Math.max(60, median(seg));
    }
    const blend = clamp(corrRef / w1, 0.35, 2.0);
    return clamp(def * (0.55 + 0.45 * blend) * (1 + (inCycle - 2) * 0.06), 0.32, 2.35);
  }

  const meas: Record<number, number> = {
    4: d01 / w1,
    5: d12 / Math.max(60, d01),
    6: d23 / Math.max(60, d01),
    7: d34 / Math.max(60, d23),
    8: d45 / Math.max(60, d01),
  };
  const m = clamp(meas[inCycle] ?? 1, 0.22, 2.85);
  const w = clamp(fibMeasuredWeight, 0, 1);
  return clamp((1 - w) * def + w * m, 0.28, 2.85);
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

function buildForwardPolylineLayer(params: {
  startStep: number;
  startTime: number;
  startPrice: number;
  maxSteps: number;
  base: number;
  /** Son mumların ATR tabanı — segment fiyat adımı uç değerlerini volatiliteye göre sıkıştırır. */
  atrFloor: number;
  cal: ProjectionCalibration;
  rates: RateProfile;
  regimeMul: number;
  isBull: boolean;
  tf: Timeframe;
  stepSec: number;
  barPeriodSec: number;
  fibMeasuredWeight: number;
  imp: ImpulseCountV2;
  postAbc: CorrectiveCountV2 | null;
  lineColor: string | undefined;
  zigzagKind: "elliott_projection" | "elliott_projection_alt";
  markerSuffix: string;
}): PatternLayerOverlay {
  const points: Pt[] = [{ time: params.startTime as UTCTimestamp, value: params.startPrice }];
  const markers: SeriesMarker<UTCTimestamp>[] = [];
  let cur = params.startPrice;
  let t = params.startTime as number;
  const markerColor =
    params.lineColor && /^#[0-9A-Fa-f]{3,8}$/.test(params.lineColor.trim())
      ? params.lineColor.trim()
      : "#64b5f6";

  for (let i = 0; i < params.maxSteps; i++) {
    const stepNo = params.startStep + i;
    const magRaw = stepMagnitudeWithCalibration(stepNo, params.base, params.cal);
    const stepRateBase = stepIsCorrective(stepNo) ? params.rates.corrRate : params.rates.motiveRate;
    const stepRate = Math.max(1e-12, stepRateBase * params.regimeMul);
    const magNominal = stepRate * params.stepSec;
    const loB = magNominal * 0.42;
    const hiB = magNominal * 1.88;
    const af = params.atrFloor;
    const mag =
      af > 1e-12
        ? clamp(magRaw, Math.max(loB, af * 0.1), Math.min(hiB, af * 5.5))
        : clamp(magRaw, loB, hiB);
    const dir = stepDirection(stepNo);
    const signed = (params.isBull ? 1 : -1) * dir * mag;
    cur += signed;
    const deltaPrice = Math.abs(signed);
    const dtRaw = deltaPrice / stepRate;
    const inCycle = ((stepNo - 1) % 8) + 1;
    const fibT = projectionFibTimeMultiplier(inCycle, params.imp, params.postAbc, params.fibMeasuredWeight);
    const minDt = Math.max(45, Math.round(params.stepSec * 0.28), params.barPeriodSec);
    const maxDt = Math.max(minDt + 1, Math.round(params.stepSec * 7.5));
    t += Math.round(clamp(dtRaw * fibT, minDt, maxDt));
    points.push({ time: t as UTCTimestamp, value: cur });

    markers.push({
      time: t as UTCTimestamp,
      position: signed >= 0 ? "aboveBar" : "belowBar",
      shape: "circle",
      color: markerColor,
      text: `${markerLabel(stepNo, params.tf)} ${formatEtaFromStart(t - params.startTime)}${params.markerSuffix}`,
    });
  }

  return {
    upper: [],
    lower: [],
    zigzag: points,
    zigzagKind: params.zigzagKind,
    zigzagLineColor: params.lineColor,
    zigzagMarkers: markers,
  };
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

  const durationA = Math.max(60, Math.round(w1Dur * 0.618));
  const durationB = Math.max(60, Math.round(w2Dur * 1.0));
  const durationC = Math.max(60, Math.round(w1Dur * 1.0));

  const tA = (startTime + durationA) as UTCTimestamp;
  const tB = (startTime + durationA + durationB) as UTCTimestamp;
  const tC = (startTime + durationA + durationB + durationC) as UTCTimestamp;

  const aPt: FormationPoint = { time: tA, value: projAVal };
  const bPt: FormationPoint = { time: tB, value: projBVal };
  const cPt: FormationPoint = { time: tC, value: projCVal };

  // Post-ABC state
  const postPath = postAbc ? (postAbc.path?.length ? postAbc.path : postAbc.pivots) : null;
  const hasA = !!postPath && postPath.length >= 2;
  const hasB = !!postPath && postPath.length >= 3;
  const hasC = !!postPath && postPath.length >= 4;

  const w5Pt: FormationPoint = { time: p5.time as UTCTimestamp, value: p5.price };

  if (!hasA) {
    // Impulse done -> project full ABC
    layers.push(seg(w5Pt, aPt, kind, "dashed", lineColor));
    layers.push(seg(aPt, bPt, kind, "dotted", lineColor));
    layers.push(seg(bPt, cPt, kind, "dashed", lineColor));
  } else if (hasA && !hasB) {
    // A observed -> project B and C from observed A point
    const obsA = postPath![1]!;
    const obsAPt: FormationPoint = { time: obsA.time as UTCTimestamp, value: obsA.price };
    const abObs = Math.abs(obsAPt.value - p5.price);
    const projB2 = isBull ? obsAPt.value + abObs * 0.618 : obsAPt.value - abObs * 0.618;
    const projC2 = isBull ? projB2 - abObs * 1.0 : projB2 + abObs * 1.0;
    const b2: FormationPoint = { time: tB, value: projB2 };
    const c2: FormationPoint = { time: tC, value: projC2 };
    layers.push(seg(obsAPt, b2, kind, "dotted", lineColor));
    layers.push(seg(b2, c2, kind, "dashed", lineColor));
  } else if (hasA && hasB && !hasC) {
    // A and B observed -> project C from observed B
    const obsB = postPath![2]!;
    const obsBPt: FormationPoint = { time: obsB.time as UTCTimestamp, value: obsB.price };
    const aRef = postPath![1]!;
    const abObs = Math.abs(aRef.price - p5.price);
    const projC2 = isBull ? obsBPt.value - abObs * 1.0 : obsBPt.value + abObs * 1.0;
    const c2: FormationPoint = { time: tC, value: projC2 };
    layers.push(seg(obsBPt, c2, kind, "dashed", lineColor));
  } else {
    // ABC completed -> project new impulse 1-2-3-4-5 from C end (observed)
    const cEnd = postPath![postPath!.length - 1]!;
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
    layers.push(seg(p1n, p2n, kind, "dotted", lineColor));
    layers.push(seg(p2n, p3n, kind, "dashed", lineColor));
    layers.push(seg(p3n, p4n, kind, "dotted", lineColor));
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

/**
 * Lightweight V2 forward projection:
 * - `sourceTf` doluysa o TF itkisi kullanılır; çizim çapası **aynı TF’in** `ohlcByTf` son mumu olmalı
 *   (ana grafik farklı intervaldeyse `anchorRows` son mumu itkı ile yanlış hizalanırdı).
 * - Segment süresi: `Δt ≈ |Δfiyat| / hız` (itkı/düzeltme için ölçülen pivot hızları + güncel mum volatilitesi).
 * - Genlik: itkı 1/3/5 ortalaması + son 14 mum ATR karışımı; segment fiyat adımı ATR ile alt/üst sıkıştırılır.
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
  const fibMeasuredWeight =
    typeof opt.fibMeasuredWeight === "number" && Number.isFinite(opt.fibMeasuredWeight) ? opt.fibMeasuredWeight : 0.5;

  const base = blendedProjectionPriceBase(imp, rowsForStep);
  const atrFloor = inferAtr14(rowsForStep);

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
  const calBase = buildCalibrationFromPostAbc(postAbc, base);
  const cal0 = applyFormationCalibrationTweak(postAbc, calBase);
  const rates = buildRateProfile(imp, postAbc);
  const refBlendRate = (rates.motiveRate + rates.corrRate) * 0.5;
  const regimeMul = recentVsImpulseVelocityMul(rowsForStep, refBlendRate);

  const layers: PatternLayerOverlay[] = [];
  const showAlt = opt.includeAltScenario !== false;
  const mode = opt.mode ?? "legacy";

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

  const polyShared = {
    startStep,
    startTime,
    startPrice,
    maxSteps,
    base,
    atrFloor,
    rates,
    regimeMul,
    isBull,
    tf,
    stepSec,
    barPeriodSec,
    fibMeasuredWeight,
    imp,
    postAbc,
    lineColor,
  };

  if (mode === "formation") {
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
  } else {
    if (showAlt) {
      layers.push(
        buildForwardPolylineLayer({
          ...polyShared,
          cal: extendedThirdWaveCalibration(cal0),
          zigzagKind: "elliott_projection_alt",
          markerSuffix: " \u203b",
        }),
      );
    }

    layers.push(
      buildForwardPolylineLayer({
        ...polyShared,
        cal: cal0,
        zigzagKind: "elliott_projection",
        markerSuffix: "",
      }),
    );
  }

  return {
    layers,
  };
}

