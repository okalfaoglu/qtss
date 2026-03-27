import type { SeriesMarker, UTCTimestamp } from "lightweight-charts";
import {
  DEFAULT_ELLIOTT_MTF_WAVE_COLORS,
  type ElliottMtfWaveColors,
} from "../elliottWaveAppConfig";
import { DEFAULT_ELLIOTT_PATTERN_MENU, type ElliottPatternMenuToggles } from "../elliottPatternMenuCatalog";
import type { PatternLayerOverlay } from "../patternDrawingBatchOverlay";
import type { CorrectiveCountV2, ElliottEngineOutputV2, ImpulseCountV2, TimeframeStateV2, ZigzagPivot } from "./types";
import { extendZigzagPivotsForChartLine } from "./zigzag";

function mergePatternMenu(m?: ElliottPatternMenuToggles): ElliottPatternMenuToggles {
  return { ...DEFAULT_ELLIOTT_PATTERN_MENU, ...m };
}

function showImpulseOverlay(menu: ElliottPatternMenuToggles, impulse: ImpulseCountV2): boolean {
  const v = impulse.variant ?? "standard";
  if (v === "diagonal") return menu.motive_diagonal;
  return menu.motive_impulse;
}

function showCorrectiveOverlay(menu: ElliottPatternMenuToggles, c: CorrectiveCountV2): boolean {
  switch (c.pattern) {
    case "zigzag":
      return menu.corrective_zigzag;
    case "flat":
      return menu.corrective_flat;
    case "triangle":
      return menu.corrective_triangle;
    case "combination":
      return menu.corrective_complex_wxy;
    case "abc":
      return menu.corrective_zigzag || menu.corrective_flat;
    default:
      return true;
  }
}

type Pt = { time: UTCTimestamp; value: number };

function toPts(p: ZigzagPivot[]): Pt[] {
  return p.map((x) => ({ time: x.time as UTCTimestamp, value: x.price }));
}

function impulseLabelsByTf(tf: "4h" | "1h" | "15m"): string[] {
  if (tf === "4h") return ["①", "②", "③", "④", "⑤"];
  if (tf === "1h") return ["(1)", "(2)", "(3)", "(4)", "(5)"];
  return ["i", "ii", "iii", "iv", "v"];
}

function toCircledUpperLetter(ch: string): string {
  const code = ch.toUpperCase().charCodeAt(0);
  if (code < 65 || code > 90) return ch.toUpperCase();
  return String.fromCharCode(9398 + (code - 65)); // Ⓐ..Ⓩ
}

function correctiveSymbolByTf(tf: "4h" | "1h" | "15m", raw: string): string {
  const t = (raw ?? "").trim();
  if (!t) return "a";
  if (tf === "4h") return toCircledUpperLetter(t[0] ?? "A");
  if (tf === "1h") return `(${(t[0] ?? "A").toUpperCase()})`;
  return (t[0] ?? "a").toLowerCase();
}

function correctiveRolePrefix(role: "wave2" | "wave4" | "post", tf: "4h" | "1h" | "15m"): string {
  if (role === "post") return "+";
  if (role === "wave2") return tf === "4h" ? "②" : tf === "1h" ? "(2)" : "ii";
  return tf === "4h" ? "④" : tf === "1h" ? "(4)" : "iv";
}

function waveLabels(
  tf: "4h" | "1h" | "15m",
  s: TimeframeStateV2,
  wc: ElliottMtfWaveColors,
): SeriesMarker<UTCTimestamp>[] {
  if (!s.impulse) return [];
  const [, p1, p2, p3, p4, p5] = s.impulse.pivots;
  const names = impulseLabelsByTf(tf);
  const pts = [p1, p2, p3, p4, p5];
  const color = wc[tf];
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
  if (text.startsWith("+")) return 2;
  if (text.startsWith("(2)") || text.startsWith("(4)")) return 1;
  return 0;
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
    const texts = [...new Set(arr.map((x) => x.text).filter(Boolean))] as string[];
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
  tf: "4h" | "1h" | "15m",
  wc: ElliottMtfWaveColors,
): SeriesMarker<UTCTimestamp>[] {
  const path = c.path?.length ? c.path : c.pivots;
  const labels = c.labels?.length ? c.labels : ["a", "b", "c"];
  const pts = path.slice(1); // skip start
  const n = labels.length;
  const prefix = correctiveRolePrefix(role, tf);
  const color = wc[tf];
  return pts.map((p, i) => {
    const li = n < 1 ? "a" : i < n ? labels[i]! : labels[i % n]!;
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
  patternMenu?: ElliottPatternMenuToggles,
  waveColors?: ElliottMtfWaveColors,
  showHistorical = false,
): {
  layers: PatternLayerOverlay[];
  waveLabels: SeriesMarker<UTCTimestamp>[];
} {
  const menu = mergePatternMenu(patternMenu);
  const wc = waveColors ?? DEFAULT_ELLIOTT_MTF_WAVE_COLORS;
  const layers: PatternLayerOverlay[] = [];
  const labels: SeriesMarker<UTCTimestamp>[] = [];

  const map: Array<{ tf: "4h" | "1h" | "15m"; kind: PatternLayerOverlay["zigzagKind"] }> = [
    { tf: "4h", kind: "elliott_v2_macro" },
    { tf: "1h", kind: "elliott_v2_intermediate" },
    { tf: "15m", kind: "elliott_v2_micro" },
  ];
  const histMap: Array<{ tf: "4h" | "1h" | "15m"; kind: PatternLayerOverlay["zigzagKind"] }> = [
    { tf: "4h", kind: "elliott_v2_hist_macro" },
    { tf: "1h", kind: "elliott_v2_hist_intermediate" },
    { tf: "15m", kind: "elliott_v2_hist_micro" },
  ];
  const zigMap: Array<{ tf: "4h" | "1h" | "15m"; kind: PatternLayerOverlay["zigzagKind"] }> = [
    { tf: "4h", kind: "elliott_v2_zigzag_macro" },
    { tf: "1h", kind: "elliott_v2_zigzag_intermediate" },
    { tf: "15m", kind: "elliott_v2_zigzag_micro" },
  ];

  for (const { tf, kind } of zigMap) {
    const s = out.states[tf];
    if (!s?.pivots?.length || s.pivots.length < 2) continue;
    const rows = out.ohlcByTf?.[tf];
    const pivotsForLine =
      rows?.length && s.pivots.length >= 2
        ? extendZigzagPivotsForChartLine(rows, s.pivots, out.zigzagParams)
        : s.pivots;
    layers.push({
      upper: [],
      lower: [],
      zigzag: toPts(pivotsForLine),
      zigzagKind: kind,
      zigzagLineColor: wc[tf],
    });
  }

  for (const { tf, kind } of map) {
    const s = out.states[tf];
    if (!s) continue;
    if (s.impulse && showImpulseOverlay(menu, s.impulse)) {
      layers.push({
        upper: [],
        lower: [],
        zigzag: toPts(s.impulse.pivots),
        zigzagKind: kind,
        zigzagLineColor: wc[tf],
      });
      labels.push(...waveLabels(tf, s, wc));
    }
    if (s.wave2 && showCorrectiveOverlay(menu, s.wave2)) {
      const p = s.wave2.path?.length ? s.wave2.path : s.wave2.pivots;
      layers.push({ upper: [], lower: [], zigzag: toPts(p), zigzagKind: kind, zigzagLineColor: wc[tf] });
      labels.push(...correctiveLabels(s.wave2, "wave2", tf, wc));
    }
    if (s.wave4 && showCorrectiveOverlay(menu, s.wave4)) {
      const p = s.wave4.path?.length ? s.wave4.path : s.wave4.pivots;
      layers.push({ upper: [], lower: [], zigzag: toPts(p), zigzagKind: kind, zigzagLineColor: wc[tf] });
      labels.push(...correctiveLabels(s.wave4, "wave4", tf, wc));
    }
    if (s.postImpulseAbc && showCorrectiveOverlay(menu, s.postImpulseAbc)) {
      const p = s.postImpulseAbc.path?.length ? s.postImpulseAbc.path : s.postImpulseAbc.pivots;
      layers.push({ upper: [], lower: [], zigzag: toPts(p), zigzagKind: kind, zigzagLineColor: wc[tf] });
      labels.push(...correctiveLabels(s.postImpulseAbc, "post", tf, wc));
    }
  }

  if (showHistorical) {
    for (const { tf, kind } of histMap) {
      const s = out.states[tf];
      if (!s?.historicalImpulses?.length) continue;
      const mainStart = s.impulse?.pivots[0]?.index ?? Number.NaN;
      const mainEnd = s.impulse?.pivots[5]?.index ?? Number.NaN;
      for (const hi of s.historicalImpulses) {
        const hs = hi.pivots[0].index;
        const he = hi.pivots[5].index;
        if (hs === mainStart && he === mainEnd) continue;
        if (!showImpulseOverlay(menu, hi)) continue;
        layers.push({
          upper: [],
          lower: [],
          zigzag: toPts(hi.pivots),
          zigzagKind: kind,
          zigzagLineColor: wc[tf],
        });
      }
    }
  }

  labels.sort((a, b) => (a.time as number) - (b.time as number));
  return { layers, waveLabels: mergeMarkersAtSameTime(labels) };
}

