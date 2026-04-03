import type { SeriesMarker, UTCTimestamp } from "lightweight-charts";
import {
  DEFAULT_ELLIOTT_MTF_WAVE_COLORS,
  type ElliottMtfWaveColors,
  type ElliottPatternMenuByTf,
} from "../elliottWaveAppConfig";
import {
  correctiveCombinationIsTriple,
  DEFAULT_ELLIOTT_PATTERN_MENU,
  type ElliottPatternMenuToggles,
} from "../elliottPatternMenuCatalog";
import { patternMenuAllowsFlatAbc, patternMenuAllowsZigzagAbc } from "./corrective";
import type { PatternLayerOverlay } from "../patternDrawingBatchOverlay";
import type {
  CorrectiveCountV2,
  ElliottEngineOutputV2,
  ImpulseCountV2,
  Timeframe,
  TimeframeStateV2,
  ZigzagPivot,
} from "./types";
import { extendZigzagPivotsForChartLine } from "./zigzag";

function mergePatternMenu(m?: ElliottPatternMenuToggles): ElliottPatternMenuToggles {
  return { ...DEFAULT_ELLIOTT_PATTERN_MENU, ...m };
}

function showImpulseOverlay(menu: ElliottPatternMenuToggles, impulse: ImpulseCountV2): boolean {
  const v = impulse.variant ?? "standard";
  if (v === "diagonal") {
    const role = impulse.diagonalRole ?? "unknown";
    if (role === "leading") return menu.motive_diagonal_leading;
    if (role === "ending") return menu.motive_diagonal_ending;
    return menu.motive_diagonal_leading || menu.motive_diagonal_ending;
  }
  return menu.motive_impulse;
}

function showCorrectiveOverlay(menu: ElliottPatternMenuToggles, c: CorrectiveCountV2): boolean {
  switch (c.pattern) {
    case "zigzag":
      return patternMenuAllowsZigzagAbc(menu);
    case "flat":
      return patternMenuAllowsFlatAbc(menu);
    case "triangle":
      return menu.corrective_triangle;
    case "combination":
      return correctiveCombinationIsTriple(c)
        ? menu.corrective_complex_triple
        : menu.corrective_complex_double;
    case "abc":
      return menu.corrective_zigzag || menu.corrective_flat;
    default:
      return true;
  }
}

type Pt = { time: UTCTimestamp; value: number };
type TfTriBool = Record<Timeframe, boolean>;
type TfTriStyle = Record<Timeframe, "solid" | "dotted" | "dashed">;
type TfTriWidth = Record<Timeframe, number>;

const CHART_TF_ORDER: Timeframe[] = ["1w", "1d", "4h", "1h", "15m"];

function triBoolDefault(on: boolean): TfTriBool {
  return { "1w": on, "1d": on, "4h": on, "1h": on, "15m": on };
}

function triStyleDefault(
  a: "solid" | "dotted" | "dashed",
  b: "solid" | "dotted" | "dashed",
  c: "solid" | "dotted" | "dashed",
  d: "solid" | "dotted" | "dashed",
  e: "solid" | "dotted" | "dashed",
): TfTriStyle {
  return { "1w": a, "1d": b, "4h": c, "1h": d, "15m": e };
}

function triWidthDefault(a: number, b: number, c: number, d: number, e: number): TfTriWidth {
  return { "1w": a, "1d": b, "4h": c, "1h": d, "15m": e };
}

function toPts(p: ZigzagPivot[]): Pt[] {
  return p.map((x) => ({ time: x.time as UTCTimestamp, value: x.price }));
}

/**
 * Post-impulse ABC (+a/+b/+c): engine bazen mikro zigzag için `path` üretir; çizgi iç içe ve okunaksız olur.
 * Köşe sayısı yeterliyse ve path daha uzunsa, görüntü için yalnızca `pivots` (A–B–C köşeleri) kullanılır.
 */
function postImpulseAbcLinePivots(c: CorrectiveCountV2): ZigzagPivot[] {
  const path = c.path?.length ? c.path : c.pivots;
  if (c.pivots.length >= 4 && path.length > c.pivots.length) {
    return [...c.pivots];
  }
  return path;
}

/**
 * Map `labels` to pivots without repeating: double W–X–Y keeps many vertices in `path` but only three
 * structural corners (after start) in `pivots`; triangle / triple match `path.length - 1 === labels.length`.
 */
export function correctiveLabelAnchors(c: CorrectiveCountV2): { pts: ZigzagPivot[]; labels: string[] } {
  const labels = c.labels?.length ? c.labels : ["a", "b", "c"];
  const { pivots, path } = c;
  if (pivots.length >= 2 && pivots.length - 1 === labels.length) {
    return { pts: [...pivots.slice(1)], labels: [...labels] };
  }
  if (path?.length && path.length - 1 === labels.length) {
    return { pts: [...path.slice(1)], labels: [...labels] };
  }
  const raw = path?.length ? path : pivots;
  const tail = raw.slice(1);
  const n = Math.min(tail.length, labels.length);
  return { pts: tail.slice(0, n), labels: labels.slice(0, n) };
}

function impulseLabelsByTf(tf: Timeframe): string[] {
  if (tf === "1w") return ["\u2160", "\u2161", "\u2162", "\u2163", "\u2164"];
  if (tf === "1d" || tf === "4h") return ["①", "②", "③", "④", "⑤"];
  if (tf === "1h") return ["(1)", "(2)", "(3)", "(4)", "(5)"];
  return ["i", "ii", "iii", "iv", "v"];
}

function toCircledUpperLetter(ch: string): string {
  const code = ch.toUpperCase().charCodeAt(0);
  if (code < 65 || code > 90) return ch.toUpperCase();
  return String.fromCharCode(9398 + (code - 65)); // Ⓐ..Ⓩ
}

function correctiveSymbolByTf(tf: Timeframe, raw: string): string {
  const t = (raw ?? "").trim();
  if (!t) return "a";
  if (tf === "15m") return (t[0] ?? "a").toLowerCase();
  if (tf === "1h") return `(${(t[0] ?? "A").toUpperCase()})`;
  return toCircledUpperLetter(t[0] ?? "A");
}

function correctiveRolePrefix(role: "wave2" | "wave4" | "post", tf: Timeframe): string {
  if (role === "post") return "+";
  if (role === "wave2") return tf === "1h" ? "(2)" : tf === "15m" ? "ii" : "②";
  return tf === "1h" ? "(4)" : tf === "15m" ? "iv" : "④";
}

/** Ana dalga rengini açarak iç (dalga 1 alt itkı) çizgisini ayırır. */
function blendTowardWhite(hex: string, t: number): string {
  const h = hex.replace(/^#/, "");
  if (h.length !== 6 || !/^[0-9a-fA-F]{6}$/.test(h)) return hex;
  const mix = (c: number) => Math.round(c + (255 - c) * t);
  const r = mix(parseInt(h.slice(0, 2), 16));
  const g = mix(parseInt(h.slice(2, 4), 16));
  const b = mix(parseInt(h.slice(4, 6), 16));
  return `#${r.toString(16).padStart(2, "0")}${g.toString(16).padStart(2, "0")}${b.toString(16).padStart(2, "0")}`;
}

/** Alt itkı bacak uçları (dalga 1 / 3 / 5). */
function nestedImpulseMarkers(
  nested: ImpulseCountV2,
  names: [string, string, string, string, string],
  wc: ElliottMtfWaveColors,
  labelColors: ElliottMtfWaveColors,
  tf: Timeframe,
): SeriesMarker<UTCTimestamp>[] {
  const [, p1, p2, p3, p4, p5] = nested.pivots;
  const pts = [p1, p2, p3, p4, p5];
  const color = labelColors[tf] ?? wc[tf];
  return pts.map((p, i) => ({
    time: p.time as UTCTimestamp,
    position: p.kind === "high" ? "aboveBar" : "belowBar",
    shape: "circle" as const,
    color,
    text: names[i],
  }));
}

/** Dalga 2 / 4 mikro düzeltme etiketleri (`w2·a` benzeri). */
function correctiveNestedLegLabels(
  c: CorrectiveCountV2,
  role: "wave2" | "wave4",
  tf: Timeframe,
  wc: ElliottMtfWaveColors,
  labelColors: ElliottMtfWaveColors,
): SeriesMarker<UTCTimestamp>[] {
  const { pts, labels } = correctiveLabelAnchors(c);
  const prefix = role === "wave2" ? "w2·" : "w4·";
  const color = labelColors[tf] ?? wc[tf];
  return pts.map((p, i) => {
    const li = labels[i] ?? "a";
    return {
      time: p.time as UTCTimestamp,
      position: p.kind === "high" ? "aboveBar" : "belowBar",
      shape: "square",
      color,
      text: `${prefix}${correctiveSymbolByTf(tf, li)}`,
    };
  });
}

function waveLabels(
  tf: Timeframe,
  s: TimeframeStateV2,
  wc: ElliottMtfWaveColors,
  labelColors: ElliottMtfWaveColors,
): SeriesMarker<UTCTimestamp>[] {
  if (!s.impulse) return [];
  const [, p1, p2, p3, p4, p5] = s.impulse.pivots;
  const names = impulseLabelsByTf(tf);
  const pts = [p1, p2, p3, p4, p5];
  const color = labelColors[tf] ?? wc[tf];
  return pts.map((p, i) => ({
    time: p.time as UTCTimestamp,
    position: p.kind === "high" ? "aboveBar" : "belowBar",
    shape: i === 2 ? (p.kind === "high" ? "arrowUp" : "arrowDown") : "circle",
    color,
    text: names[i],
  }));
}

/** Aynı saniyede birden fazla etiket (ör. v ile +a) tek satırda birleşsin. */
function markerMergePriority(text: string): number {
  const t = (text ?? "").trim();
  if (t.startsWith("+")) return 2;
  if (t.startsWith("(2)") || t.startsWith("(4)")) return 1;
  /** Ana motive etiketleri (i…v) önce; iç düzeltme `w2·` / iç itkı `n1` sonra — okunabilirlik. */
  if (t.includes("w2·") || t.includes("w4·")) return 3;
  if (/^[nuv][1-5]$/.test(t)) return 3;
  return 0;
}

function isNestedLabel(text: string): boolean {
  const t = (text ?? "").trim();
  return t.includes("w2·") || t.includes("w4·") || /^[nuv][1-5]$/.test(t);
}

function isMainImpulseLabel(text: string): boolean {
  const t = (text ?? "").trim();
  return (
    ["\u2160", "\u2161", "\u2162", "\u2163", "\u2164"].includes(t) ||
    ["①", "②", "③", "④", "⑤"].includes(t) ||
    /^\([1-5]\)$/.test(t) ||
    ["i", "ii", "iii", "iv", "v"].includes(t)
  );
}

function mergeMarkersAtSameTime(markers: SeriesMarker<UTCTimestamp>[]): SeriesMarker<UTCTimestamp>[] {
  const groups = new Map<number, SeriesMarker<UTCTimestamp>[]>();
  for (const m of markers) {
    const t = m.time as number;
    const arr = groups.get(t) ?? [];
    arr.push(m);
    groups.set(t, arr);
  }
  const out: SeriesMarker<UTCTimestamp>[] = [];
  for (const arr of groups.values()) {
    arr.sort((a, b) => markerMergePriority(a.text ?? "") - markerMergePriority(b.text ?? ""));
    const base = arr[0]!;
    let texts = [...new Set(arr.map((x) => x.text).filter(Boolean))] as string[];
    // If a main impulse label (i..v / ①..⑤ / (1)..(5)) collides with nested labels on the same bar,
    // drop nested labels to prevent unreadable joins like `ii · w2·c`.
    if (texts.some(isMainImpulseLabel) && texts.some(isNestedLabel)) {
      texts = texts.filter((t) => !isNestedLabel(t) || t.trim().startsWith("+"));
    }
    if (texts.length <= 1) {
      out.push(base);
    } else {
      out.push({ ...base, text: texts.join(" · ") });
    }
  }
  return out.sort((a, b) => (a.time as number) - (b.time as number));
}

function correctiveLabels(
  c: CorrectiveCountV2,
  role: "wave2" | "wave4" | "post",
  tf: Timeframe,
  wc: ElliottMtfWaveColors,
  labelColors: ElliottMtfWaveColors,
): SeriesMarker<UTCTimestamp>[] {
  const { pts, labels } = correctiveLabelAnchors(c);
  const prefix = correctiveRolePrefix(role, tf);
  const color = labelColors[tf] ?? wc[tf];
  return pts.map((p, i) => {
    const li = labels[i] ?? "a";
    return {
      time: p.time as UTCTimestamp,
      position: p.kind === "high" ? "aboveBar" : "belowBar",
      shape: "square",
      color,
      text: `${prefix}${correctiveSymbolByTf(tf, li)}`,
    };
  });
}

export function v2ToChartOverlays(
  out: ElliottEngineOutputV2,
  patternMenuByTf: ElliottPatternMenuByTf | undefined,
  waveColors?: ElliottMtfWaveColors,
  styleOptions?: {
    showLines?: TfTriBool;
    showLabels?: TfTriBool;
    labelColors?: ElliottMtfWaveColors;
    lineStyles?: TfTriStyle;
    lineWidths?: TfTriWidth;
    showZigzagPivots?: TfTriBool;
    zigzagColors?: ElliottMtfWaveColors;
    zigzagLineStyles?: TfTriStyle;
    zigzagLineWidths?: TfTriWidth;
    /** Dalga 1/3/5 alt itkı ve 2/4 içi mikro düzeltme çizgileri + etiketleri. */
    showNestedFormations?: boolean;
  },
): {
  layers: PatternLayerOverlay[];
  waveLabels: SeriesMarker<UTCTimestamp>[];
} {
  const menuTf = (tf: Timeframe): ElliottPatternMenuToggles =>
    mergePatternMenu(patternMenuByTf?.[tf]);
  const wc = waveColors ?? DEFAULT_ELLIOTT_MTF_WAVE_COLORS;
  const showLines = styleOptions?.showLines ?? triBoolDefault(true);
  const showLabels = styleOptions?.showLabels ?? triBoolDefault(true);
  const labelColors = styleOptions?.labelColors ?? wc;
  const lineStyles =
    styleOptions?.lineStyles ?? triStyleDefault("solid", "solid", "solid", "dashed", "dotted");
  const lineWidths = styleOptions?.lineWidths ?? triWidthDefault(4, 4, 4, 3, 2);
  const showZigzagPivots = styleOptions?.showZigzagPivots ?? triBoolDefault(true);
  const zigzagColors = styleOptions?.zigzagColors ?? wc;
  const zigzagLineStyles =
    styleOptions?.zigzagLineStyles ?? triStyleDefault("dotted", "dotted", "dotted", "dotted", "dotted");
  const zigzagLineWidths = styleOptions?.zigzagLineWidths ?? triWidthDefault(2, 2, 2, 2, 2);
  const showNestedFormations = styleOptions?.showNestedFormations ?? true;
  const layers: PatternLayerOverlay[] = [];
  const labels: SeriesMarker<UTCTimestamp>[] = [];

  const waveKindByTf: Record<Timeframe, PatternLayerOverlay["zigzagKind"]> = {
    "1w": "elliott_v2_weekly",
    "1d": "elliott_v2_daily",
    "4h": "elliott_v2_macro",
    "1h": "elliott_v2_intermediate",
    "15m": "elliott_v2_micro",
  };
  const histKindByTf: Record<Timeframe, PatternLayerOverlay["zigzagKind"]> = {
    "1w": "elliott_v2_hist_weekly",
    "1d": "elliott_v2_hist_daily",
    "4h": "elliott_v2_hist_macro",
    "1h": "elliott_v2_hist_intermediate",
    "15m": "elliott_v2_hist_micro",
  };
  const zigKindByTf: Record<Timeframe, PatternLayerOverlay["zigzagKind"]> = {
    "1w": "elliott_v2_zigzag_weekly",
    "1d": "elliott_v2_zigzag_daily",
    "4h": "elliott_v2_zigzag_macro",
    "1h": "elliott_v2_zigzag_intermediate",
    "15m": "elliott_v2_zigzag_micro",
  };
  const map = CHART_TF_ORDER.map((tf) => ({ tf, kind: waveKindByTf[tf] }));
  const histMap = CHART_TF_ORDER.map((tf) => ({ tf, kind: histKindByTf[tf] }));
  const zigMap = CHART_TF_ORDER.map((tf) => ({ tf, kind: zigKindByTf[tf] }));

  for (const { tf, kind } of zigMap) {
    if (!showZigzagPivots[tf]) continue;
    const s = out.states[tf];
    if (!s?.pivots?.length || s.pivots.length < 2) continue;
    const rows = out.ohlcByTf?.[tf];
    const pivotsForLine =
      rows?.length && s.pivots.length >= 2
        ? extendZigzagPivotsForChartLine(rows, s.pivots, out.zigzagParamsByTf?.[tf] ?? out.zigzagParams)
        : s.pivots;
    layers.push({
      upper: [],
      lower: [],
      zigzag: toPts(pivotsForLine),
      zigzagKind: kind,
      zigzagLineColor: zigzagColors[tf],
      zigzagLineStyle: zigzagLineStyles[tf],
      zigzagLineWidth: zigzagLineWidths[tf],
    });
  }

  for (const { tf, kind } of map) {
    const s = out.states[tf];
    if (!s) continue;
    const m = menuTf(tf);
    if (showLines[tf] && s.impulse && showImpulseOverlay(m, s.impulse)) {
      layers.push({
        upper: [],
        lower: [],
        zigzag: toPts(s.impulse.pivots),
        zigzagKind: kind,
        zigzagLineColor: wc[tf],
        zigzagLineStyle: lineStyles[tf],
        zigzagLineWidth: lineWidths[tf],
      });
      if (showLabels[tf]) labels.push(...waveLabels(tf, s, wc, labelColors));
    }
    const pushNestedImpulse = (imp: ImpulseCountV2 | null | undefined, label5: [string, string, string, string, string]) => {
      if (!showNestedFormations || !showLines[tf] || !imp || !showImpulseOverlay(m, imp)) return;
      layers.push({
        upper: [],
        lower: [],
        zigzag: toPts(imp.pivots),
        zigzagKind: kind,
        zigzagLineColor: blendTowardWhite(wc[tf], 0.38),
        zigzagLineStyle: "dashed",
        zigzagLineWidth: Math.max(1, lineWidths[tf] - 1),
      });
      if (showLabels[tf]) labels.push(...nestedImpulseMarkers(imp, label5, wc, labelColors, tf));
    };
    pushNestedImpulse(s.wave1NestedImpulse, ["n1", "n2", "n3", "n4", "n5"]);
    if (showLines[tf] && s.wave2 && showCorrectiveOverlay(m, s.wave2)) {
      const p = s.wave2.path?.length ? s.wave2.path : s.wave2.pivots;
      layers.push({
        upper: [],
        lower: [],
        zigzag: toPts(p),
        zigzagKind: kind,
        zigzagLineColor: wc[tf],
        zigzagLineStyle: lineStyles[tf],
        zigzagLineWidth: lineWidths[tf],
      });
      if (showLabels[tf]) labels.push(...correctiveLabels(s.wave2, "wave2", tf, wc, labelColors));
    }
    if (
      showNestedFormations &&
      showLines[tf] &&
      s.wave2NestedCorrective &&
      showCorrectiveOverlay(m, s.wave2NestedCorrective)
    ) {
      const p = s.wave2NestedCorrective.path?.length ? s.wave2NestedCorrective.path : s.wave2NestedCorrective.pivots;
      layers.push({
        upper: [],
        lower: [],
        zigzag: toPts(p),
        zigzagKind: kind,
        zigzagLineColor: blendTowardWhite(wc[tf], 0.42),
        zigzagLineStyle: "dashed",
        zigzagLineWidth: Math.max(1, lineWidths[tf] - 1),
      });
      if (showLabels[tf]) labels.push(...correctiveNestedLegLabels(s.wave2NestedCorrective, "wave2", tf, wc, labelColors));
    }
    pushNestedImpulse(s.wave3NestedImpulse, ["u1", "u2", "u3", "u4", "u5"]);
    if (showLines[tf] && s.wave4 && showCorrectiveOverlay(m, s.wave4)) {
      const p = s.wave4.path?.length ? s.wave4.path : s.wave4.pivots;
      layers.push({
        upper: [],
        lower: [],
        zigzag: toPts(p),
        zigzagKind: kind,
        zigzagLineColor: wc[tf],
        zigzagLineStyle: lineStyles[tf],
        zigzagLineWidth: lineWidths[tf],
      });
      if (showLabels[tf]) labels.push(...correctiveLabels(s.wave4, "wave4", tf, wc, labelColors));
    }
    if (
      showNestedFormations &&
      showLines[tf] &&
      s.wave4NestedCorrective &&
      showCorrectiveOverlay(m, s.wave4NestedCorrective)
    ) {
      const p = s.wave4NestedCorrective.path?.length ? s.wave4NestedCorrective.path : s.wave4NestedCorrective.pivots;
      layers.push({
        upper: [],
        lower: [],
        zigzag: toPts(p),
        zigzagKind: kind,
        zigzagLineColor: blendTowardWhite(wc[tf], 0.42),
        zigzagLineStyle: "dashed",
        zigzagLineWidth: Math.max(1, lineWidths[tf] - 1),
      });
      if (showLabels[tf]) labels.push(...correctiveNestedLegLabels(s.wave4NestedCorrective, "wave4", tf, wc, labelColors));
    }
    pushNestedImpulse(s.wave5NestedImpulse, ["v1", "v2", "v3", "v4", "v5"]);
    if (showLines[tf] && s.postImpulseAbc && showCorrectiveOverlay(m, s.postImpulseAbc)) {
      const linePivots = postImpulseAbcLinePivots(s.postImpulseAbc);
      layers.push({
        upper: [],
        lower: [],
        zigzag: toPts(linePivots),
        zigzagKind: "elliott_v2_post_abc",
        zigzagLineColor: wc[tf],
        zigzagLineStyle: lineStyles[tf],
        zigzagLineWidth: Math.max(1, lineWidths[tf] - 1),
      });
      if (showLabels[tf]) labels.push(...correctiveLabels(s.postImpulseAbc, "post", tf, wc, labelColors));
    }
  }

  for (const { tf, kind } of histMap) {
    if (!showLines[tf]) continue;
    const s = out.states[tf];
    if (!s?.historicalImpulses?.length) continue;
    const mainStart = s.impulse?.pivots[0]?.index ?? Number.NaN;
    const mainEnd = s.impulse?.pivots[5]?.index ?? Number.NaN;
    const m = menuTf(tf);
    const waveKind = waveKindByTf[tf];
    const extras = s.historicalImpulseExtras ?? [];
    for (let hiIdx = 0; hiIdx < s.historicalImpulses.length; hiIdx++) {
      const hi = s.historicalImpulses[hiIdx]!;
      const hs = hi.pivots[0].index;
      const he = hi.pivots[5].index;
      if (hs === mainStart && he === mainEnd) continue;
      if (!showImpulseOverlay(m, hi)) continue;
      layers.push({
        upper: [],
        lower: [],
        zigzag: toPts(hi.pivots),
        zigzagKind: kind,
        zigzagLineColor: wc[tf],
        zigzagLineStyle: lineStyles[tf],
        zigzagLineWidth: lineWidths[tf],
      });
      if (showLabels[tf]) labels.push(...waveLabels(tf, { ...s, impulse: hi }, wc, labelColors));

      const ex = extras[hiIdx];
      if (!ex) continue;

      if (showLines[tf] && ex.wave2 && showCorrectiveOverlay(m, ex.wave2)) {
        const p = ex.wave2.path?.length ? ex.wave2.path : ex.wave2.pivots;
        layers.push({
          upper: [],
          lower: [],
          zigzag: toPts(p),
          zigzagKind: waveKind,
          zigzagLineColor: wc[tf],
          zigzagLineStyle: lineStyles[tf],
          zigzagLineWidth: lineWidths[tf],
        });
        if (showLabels[tf]) labels.push(...correctiveLabels(ex.wave2, "wave2", tf, wc, labelColors));
      }
      if (showLines[tf] && ex.wave4 && showCorrectiveOverlay(m, ex.wave4)) {
        const p = ex.wave4.path?.length ? ex.wave4.path : ex.wave4.pivots;
        layers.push({
          upper: [],
          lower: [],
          zigzag: toPts(p),
          zigzagKind: waveKind,
          zigzagLineColor: wc[tf],
          zigzagLineStyle: lineStyles[tf],
          zigzagLineWidth: lineWidths[tf],
        });
        if (showLabels[tf]) labels.push(...correctiveLabels(ex.wave4, "wave4", tf, wc, labelColors));
      }
      if (showLines[tf] && ex.postImpulseAbc && showCorrectiveOverlay(m, ex.postImpulseAbc)) {
        const linePivots = postImpulseAbcLinePivots(ex.postImpulseAbc);
        layers.push({
          upper: [],
          lower: [],
          zigzag: toPts(linePivots),
          zigzagKind: "elliott_v2_post_abc",
          zigzagLineColor: wc[tf],
          zigzagLineStyle: lineStyles[tf],
          zigzagLineWidth: Math.max(1, lineWidths[tf] - 1),
        });
        if (showLabels[tf]) labels.push(...correctiveLabels(ex.postImpulseAbc, "post", tf, wc, labelColors));
      }
    }
  }

  labels.sort((a, b) => (a.time as number) - (b.time as number));
  return { layers, waveLabels: mergeMarkersAtSameTime(labels) };
}

