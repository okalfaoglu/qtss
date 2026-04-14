/**
 * Chart.tsx — TradingView lightweight-charts v5 based chart page.
 *
 * Replaces the old SVG-based chart with a proper canvas-rendered chart
 * that handles price scale, time scale, crosshair, zoom, pan natively.
 *
 * All detection overlays, zigzag, Wyckoff, volume, Entry/TP/SL are
 * preserved and rendered via TV primitives / markers / line series.
 */

import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  createChart,
  CandlestickSeries,
  HistogramSeries,
  LineSeries,
  type IChartApi,
  type ISeriesApi,
  type CandlestickData,
  type Time,
  type SeriesMarker,
  type DeepPartial,
  type ChartOptions,
  ColorType,
  CrosshairMode,
  LineStyle,
  type HistogramData,
  createSeriesMarkers,
} from "lightweight-charts";

import { apiFetch } from "../lib/api";
import { RectanglePrimitive, type RectangleOptions } from "../lib/rectangle-primitive";
import type { CandleBar, ChartWorkspace, DetectionOverlay } from "../lib/types";

// ─── Constants ───────────────────────────────────────────────────────
const DEFAULTS = { venue: "binance", symbol: "BTCUSDT", timeframe: "1h" };
const PAGE_SIZE = 500;
const PREFETCH_THRESHOLD = 50;

const FAMILY_COLORS: Record<string, string> = {
  elliott: "#7dd3fc",
  harmonic: "#f472b6",
  classical: "#facc15",
  wyckoff: "#a78bfa",
  range: "#5eead4",
  tbm: "#fb923c",
  custom: "#d4d4d8",
};

const RANGE_SUBKIND_COLORS: Record<string, string> = {
  bullish_fvg: "#34d399",
  bearish_fvg: "#f87171",
  bullish_ob: "#60a5fa",
  bearish_ob: "#fb923c",
  liquidity_pool_high: "#facc15",
  liquidity_pool_low: "#facc15",
  equal_highs: "#c084fc",
  equal_lows: "#c084fc",
};

const ZONE_BOX_SUBKINDS = new Set([
  "bullish_fvg", "bearish_fvg",
  "bullish_ob", "bearish_ob",
  "liquidity_pool_high", "liquidity_pool_low",
  "equal_highs", "equal_lows",
]);

// Labels reserved for future tooltip / legend rendering.
// const RANGE_SUBKIND_LABELS: Record<string, string> = { ... };

const TIMEFRAMES = ["1m", "3m", "5m", "15m", "30m", "1h", "4h", "1d", "1w", "1M"];
const ZIGZAG_PCT = 0.01;

// ─── Toolbar Tools ───────────────────────────────────────────────────
type ToolId = "crosshair" | "trendline" | "hline" | "fibretracement" | "measure";
interface ToolDef {
  id: ToolId;
  label: string;
  icon: string;
  tip: string;
}
const TOOLS: ToolDef[] = [
  { id: "crosshair", label: "Crosshair", icon: "⊕", tip: "Crosshair" },
  { id: "trendline", label: "Trend Line", icon: "╱", tip: "Trend çizgisi" },
  { id: "hline", label: "H-Line", icon: "─", tip: "Yatay çizgi" },
  { id: "fibretracement", label: "Fib Ret", icon: "φ", tip: "Fibonacci geri çekilme" },
  { id: "measure", label: "Measure", icon: "⊞", tip: "Ölçüm aracı" },
];

// ─── Family Mode ─────────────────────────────────────────────────────
type FamilyMode = "off" | "on" | "detail";

const DETAIL_SUB_BUTTONS = [
  { key: "entry_tp_sl", label: "Entry / TP / SL", icon: "⊞" },
  { key: "labels", label: "Labels", icon: "Aa" },
  { key: "fib_levels", label: "Fib Levels", icon: "φ" },
  { key: "measured_move", label: "Measured Move", icon: "⟷" },
  { key: "invalidation", label: "Invalidation Zone", icon: "✕" },
];

// ─── Helpers ─────────────────────────────────────────────────────────
function isoToUnix(iso: string): Time {
  return Math.floor(new Date(iso).getTime() / 1000) as Time;
}

/** Sort + deduplicate line data for TV strict-ascending requirement */
function sortLineData<T extends { time: Time }>(data: T[]): T[] {
  const sorted = [...data].sort((a, b) => (a.time as number) - (b.time as number));
  // Deduplicate: when two points share the same timestamp, keep only the LAST
  // one (lightweight-charts requires strictly ascending time). For wave overlays
  // where order matters, callers should avoid sortLineData and use
  // dedupeLineData instead.
  const out: T[] = [];
  let prev = -1;
  for (const d of sorted) {
    const t = d.time as number;
    if (t <= prev) continue;
    prev = t;
    out.push(d);
  }
  return out;
}

/** For wave polylines: preserve anchor order, only bump duplicate timestamps
 *  by +1s so lightweight-charts accepts them. No re-sorting. */
function dedupeLineData<T extends { time: Time }>(data: T[]): T[] {
  const out: T[] = [];
  let prev = -1;
  for (const d of data) {
    let t = d.time as number;
    if (t <= prev) t = prev + 1;
    prev = t;
    out.push({ ...d, time: t as Time });
  }
  return out;
}

// ─── Elliott Wave Degree System ──────────────────────────────────────
// Frost & Prechter dalga dereceleri — timeframe + pivot level → degree
// Her derece kendi renk koduna sahip (profesyonel EW yazılımı standardı).
type WaveDegree =
  | "grand_supercycle"
  | "supercycle"
  | "cycle"
  | "primary"
  | "intermediate"
  | "minor"
  | "minute"
  | "minuette"
  | "subminuette";

const WAVE_DEGREE_COLORS: Record<WaveDegree, string> = {
  grand_supercycle: "#dc2626", // koyu kırmızı
  supercycle:       "#ef4444", // kırmızı
  cycle:            "#1d4ed8", // koyu mavi (navy)
  primary:          "#3b82f6", // mavi
  intermediate:     "#16a34a", // yeşil
  minor:            "#f59e0b", // turuncu
  minute:           "#8b5cf6", // mor
  minuette:         "#06b6d4", // cyan
  subminuette:      "#9ca3af", // gri
};

// Corrective has 5 entries for triangles (A-B-C-D-E). Zigzag/flat use first 3.
const WAVE_DEGREE_LABELS: Record<WaveDegree, { motive: string[]; corrective: string[] }> = {
  grand_supercycle: { motive: ["[1]","[2]","[3]","[4]","[5]"], corrective: ["[a]","[b]","[c]","[d]","[e]"] },
  supercycle:       { motive: ["(I)","(II)","(III)","(IV)","(V)"], corrective: ["(a)","(b)","(c)","(d)","(e)"] },
  cycle:            { motive: ["I","II","III","IV","V"], corrective: ["a","b","c","d","e"] },
  primary:          { motive: ["[1]","[2]","[3]","[4]","[5]"], corrective: ["[A]","[B]","[C]","[D]","[E]"] },
  intermediate:     { motive: ["(1)","(2)","(3)","(4)","(5)"], corrective: ["(A)","(B)","(C)","(D)","(E)"] },
  minor:            { motive: ["1","2","3","4","5"], corrective: ["A","B","C","D","E"] },
  minute:           { motive: ["[i]","[ii]","[iii]","[iv]","[v]"], corrective: ["[a]","[b]","[c]","[d]","[e]"] },
  minuette:         { motive: ["(i)","(ii)","(iii)","(iv)","(v)"], corrective: ["(a)","(b)","(c)","(d)","(e)"] },
  subminuette:      { motive: ["i","ii","iii","iv","v"], corrective: ["a","b","c","d","e"] },
};

// Timeframe → base degree eşlemesi (L1 pivot level baz alınır)
function waveDegreeForTimeframe(tf: string): WaveDegree {
  switch (tf) {
    case "1M":  return "cycle";
    case "1w":  return "primary";
    case "1d":  return "intermediate";
    case "4h":  return "minor";
    case "1h":  return "minute";
    case "30m": return "minuette";
    case "15m": case "5m": case "3m": case "1m":
      return "subminuette";
    default:    return "minor";
  }
}

function isMotiveElliott(subkind: string): boolean {
  return subkind.startsWith("impulse") || subkind.includes("diagonal");
}

function elliottColor(subkind: string, timeframe: string): string {
  const degree = waveDegreeForTimeframe(timeframe);
  return WAVE_DEGREE_COLORS[degree];
}

function elliottLabel(
  anchorLabel: string,
  subkind: string,
  timeframe: string,
): string {
  const degree = waveDegreeForTimeframe(timeframe);
  const isMotive = isMotiveElliott(subkind);
  const labels = isMotive
    ? WAVE_DEGREE_LABELS[degree].motive
    : WAVE_DEGREE_LABELS[degree].corrective;

  // Map anchor label (0,1,2,3,4,5 or 0,A,B,C) to degree notation
  if (isMotive) {
    const idx = parseInt(anchorLabel, 10);
    if (!isNaN(idx) && idx >= 0 && idx < labels.length) return labels[idx];
  } else {
    // Corrective: map A/B/C/D/E to degree notation
    const map: Record<string, number> = { "0": -1, A: 0, B: 1, C: 2, D: 3, E: 4 };
    const i = map[anchorLabel];
    if (i !== undefined && i >= 0 && i < labels.length) {
      return labels[i];
    }
    // Combination W-X-Y labels: pass through
    if (anchorLabel.includes("-") || anchorLabel.includes("/")) return anchorLabel;
    // For diagonal corrective (0-5 labels)
    const idx = parseInt(anchorLabel, 10);
    if (!isNaN(idx) && idx >= 0 && idx < labels.length) return labels[idx % labels.length];
  }
  return anchorLabel; // fallback
}

function familyColor(family: string, subkind?: string, timeframe?: string): string {
  if (family === "range" && subkind && RANGE_SUBKIND_COLORS[subkind]) {
    return RANGE_SUBKIND_COLORS[subkind];
  }
  if (family === "elliott" && subkind) {
    return elliottColor(subkind, timeframe ?? "1d");
  }
  return FAMILY_COLORS[family] ?? FAMILY_COLORS.custom;
}

// ─── Zigzag computation (client-side) ────────────────────────────────
type SwingLabel = "HH" | "LH" | "HL" | "LL" | null;
interface ZigzagPoint {
  time: Time;
  price: number;
  kind: "H" | "L";
  dir: number;
  swing: SwingLabel;
}

function computeZigzag(candles: CandleBar[], pct: number): ZigzagPoint[] {
  if (candles.length < 2) return [];
  const raw: Array<{ idx: number; price: number; kind: "H" | "L" }> = [];
  let dir: "up" | "down" | null = null;
  let extIdx = 0;
  let extPrice = Number(candles[0].close);
  for (let i = 1; i < candles.length; i++) {
    const hi = Number(candles[i].high);
    const lo = Number(candles[i].low);
    if (dir === null) {
      if (hi >= extPrice * (1 + pct)) {
        raw.push({ idx: extIdx, price: extPrice, kind: "L" });
        dir = "up"; extIdx = i; extPrice = hi;
      } else if (lo <= extPrice * (1 - pct)) {
        raw.push({ idx: extIdx, price: extPrice, kind: "H" });
        dir = "down"; extIdx = i; extPrice = lo;
      }
    } else if (dir === "up") {
      if (hi >= extPrice) { extIdx = i; extPrice = hi; }
      else if (lo <= extPrice * (1 - pct)) {
        raw.push({ idx: extIdx, price: extPrice, kind: "H" });
        dir = "down"; extIdx = i; extPrice = lo;
      }
    } else {
      if (lo <= extPrice) { extIdx = i; extPrice = lo; }
      else if (hi >= extPrice * (1 + pct)) {
        raw.push({ idx: extIdx, price: extPrice, kind: "L" });
        dir = "up"; extIdx = i; extPrice = hi;
      }
    }
  }
  raw.push({ idx: extIdx, price: extPrice, kind: dir === "up" ? "H" : "L" });

  let prevH: number | null = null;
  let prevL: number | null = null;
  return raw.map((p) => {
    let swing: SwingLabel = null;
    let d = 0;
    if (p.kind === "H") {
      if (prevH !== null) { swing = p.price >= prevH ? "HH" : "LH"; d = swing === "HH" ? 2 : -1; }
      prevH = p.price;
    } else {
      if (prevL !== null) { swing = p.price >= prevL ? "HL" : "LL"; d = swing === "HL" ? 1 : -2; }
      prevL = p.price;
    }
    return { time: isoToUnix(candles[p.idx].open_time), price: p.price, kind: p.kind, dir: d, swing };
  });
}

// ─── Wyckoff overlay data ────────────────────────────────────────────
interface WyckoffOverlayData {
  id: string;
  schematic: string;
  phase: string;
  confidence: number | null;
  range: { top: number | null; bottom: number | null };
  creek: number | null;
  ice: number | null;
  slope_deg: number | null;
  events: Array<{ event: string; bar_index: number; price: number; score: number }>;
  started_at: string;
}

// ─── Chart form state ────────────────────────────────────────────────
interface ChartForm {
  venue: string;
  symbol: string;
  timeframe: string;
}

// ─── Compute Entry/TP/SL from detection geometry ─────────────────────
function computeTargets(d: DetectionOverlay): {
  entry: number | null;
  tp1: number | null;
  tp2: number | null;
  sl: number | null;
} {
  const inv = Number(d.invalidation_price);
  const sl = Number.isFinite(inv) && inv > 0 ? inv : null;
  const anchors = d.anchors;
  let entry: number | null = null;
  let tp1: number | null = null;
  let tp2: number | null = null;

  if (d.subkind.includes("double_top") || d.subkind.includes("double_bottom")) {
    if (anchors.length >= 3) {
      const extreme = Number(anchors[0].price);
      const neck = Number(anchors[1].price);
      const height = Math.abs(extreme - neck);
      const dir = d.subkind.includes("bull") ? 1 : -1;
      entry = neck;
      tp1 = neck + dir * height;
      tp2 = neck + dir * height * 1.618;
    }
  } else if (d.subkind.includes("head_and_shoulders")) {
    if (anchors.length >= 5) {
      const head = Number(anchors[2].price);
      const n1 = Number(anchors[1].price);
      const n2 = Number(anchors[3].price);
      const neckline = (n1 + n2) / 2;
      const height = Math.abs(head - neckline);
      const dir = d.subkind.includes("bull") ? 1 : -1;
      entry = neckline;
      tp1 = neckline + dir * height;
      tp2 = neckline + dir * height * 1.618;
    }
  } else if (d.family === "harmonic" && anchors.length >= 5) {
    // Harmonic entry = D (PRZ), targets = CD leg retrace
    const aP = Number(anchors[1].price);
    const cP = Number(anchors[3].price);
    const dP = Number(anchors[4].price);
    const cdRange = Math.abs(cP - dP);
    // Direction: bull → price should rise from D, bear → fall from D
    // For bull: D < C (D is a low), targets are ABOVE D
    // For bear: D > C (D is a high), targets are BELOW D
    const dir = dP < cP ? 1 : -1; // derive from geometry, not subkind name
    entry = dP;
    tp1 = dP + dir * cdRange * 0.382;
    tp2 = dP + dir * cdRange * 0.618;
  } else if (d.subkind.includes("impulse") && anchors.length >= 6) {
    const p0 = Number(anchors[0].price);
    const p1 = Number(anchors[1].price);
    const p4 = Number(anchors[4].price);
    const w1h = Math.abs(p1 - p0);
    const dir = d.subkind.includes("bull") ? 1 : -1;
    entry = p4;
    tp1 = p4 + dir * w1h;
    tp2 = p4 + dir * w1h * 1.618;
  }

  return { entry, tp1, tp2, sl };
}

// ═════════════════════════════════════════════════════════════════════
// MAIN COMPONENT
// ═════════════════════════════════════════════════════════════════════

export function Chart() {
  // ─── State ───────────────────────────────────────────────────────
  const [form, setForm] = useState<ChartForm>(DEFAULTS);
  const [debounced, setDebounced] = useState<ChartForm>(DEFAULTS);
  const [olderPages, setOlderPages] = useState<ChartWorkspace[]>([]);
  const [hovered, setHovered] = useState<string | null>(null);
  const hoveredRef = useRef<string | null>(null);
  // Keep ref in sync for use inside useEffect without triggering re-render
  hoveredRef.current = hovered;
  const [showZigzag, setShowZigzag] = useState(false);
  const [showVolume, setShowVolume] = useState(true);
  const [familyModes, setFamilyModes] = useState<Record<string, FamilyMode>>({});
  const [detailLayers, setDetailLayers] = useState<Record<string, Set<string>>>({});
  const [showLabels, setShowLabels] = useState(true);
  const [showProjections, setShowProjections] = useState(true);
  const [activeTool, setActiveTool] = useState<ToolId>("crosshair");

  const fetchingOlderRef = useRef(false);
  const chartContainerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const candleSeriesRef = useRef<ISeriesApi<"Candlestick"> | null>(null);
  const volumeSeriesRef = useRef<ISeriesApi<"Histogram"> | null>(null);
  const overlayLinesRef = useRef<ISeriesApi<"Line">[]>([]);
  const rectanglePrimitivesRef = useRef<RectanglePrimitive[]>([]);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const markersRef = useRef<any>(null);
  const resizeObserverRef = useRef<ResizeObserver | null>(null);

  // ─── Derived state ──────────────────────────────────────────────
  const cycleFamily = useCallback((family: string) => {
    setFamilyModes((prev) => {
      const cur = prev[family] ?? "on";
      // Simple toggle: on ↔ off (detail via long-press / right-click)
      const next: FamilyMode = cur === "off" ? "on" : "off";
      return { ...prev, [family]: next };
    });
  }, []);

  // Long-press or right-click → detail mode
  const detailFamily = useCallback((family: string) => {
    setFamilyModes((prev) => {
      const cur = prev[family] ?? "on";
      const next: FamilyMode = cur === "detail" ? "on" : "detail";
      if (next === "detail") {
        setDetailLayers((dl) => ({ ...dl, [family]: new Set(["entry_tp_sl", "labels"]) }));
      }
      return { ...prev, [family]: next };
    });
  }, []);

  const toggleLayer = useCallback((family: string, layer: string) => {
    setDetailLayers((prev) => {
      const cur = new Set(prev[family] ?? ["entry_tp_sl"]);
      if (cur.has(layer)) cur.delete(layer); else cur.add(layer);
      return { ...prev, [family]: cur };
    });
  }, []);

  // ─── Debounce form ──────────────────────────────────────────────
  useEffect(() => {
    const t = setTimeout(() => setDebounced(form), 300);
    return () => clearTimeout(t);
  }, [form]);

  // Reset on symbol/tf change
  useEffect(() => {
    setOlderPages([]);
  }, [debounced.venue, debounced.symbol, debounced.timeframe]);

  // ─── Data queries ───────────────────────────────────────────────
  const query = useQuery({
    queryKey: ["v2", "chart", debounced],
    queryFn: () =>
      apiFetch<ChartWorkspace>(
        `/v2/chart/${debounced.venue}/${debounced.symbol}/${debounced.timeframe}?limit=${PAGE_SIZE}`,
      ),
    refetchInterval: 30_000,
    structuralSharing: true,
  });

  const wyckoffQuery = useQuery({
    queryKey: ["v2", "wyckoff", "overlay", debounced.symbol, debounced.timeframe],
    queryFn: () =>
      apiFetch<{ overlay: WyckoffOverlayData | null }>(
        `/v2/wyckoff/overlay/${debounced.symbol}/${debounced.timeframe}`,
      ),
    refetchInterval: 30_000,
  });

  // ─── Projections query ──────────────────────────────────────────
  interface ProjLeg {
    label: string;
    price_start: number;
    price_end: number;
    time_start_est: string | null;
    time_end_est: string | null;
    fib_level: string | null;
    direction: string;
  }
  interface ChartProjection {
    id: string;
    source_wave_id: string;
    alt_group: string;
    projected_kind: string;
    projected_label: string;
    direction: string;
    degree: string;
    fib_basis: string | null;
    projected_legs: ProjLeg[];
    probability: number;
    rank: number;
    state: string;
    invalidation_price: string | null;
  }
  const projectionsQuery = useQuery({
    queryKey: ["v2", "projections", debounced.venue, debounced.symbol, debounced.timeframe],
    queryFn: () =>
      apiFetch<ChartProjection[]>(
        `/v2/wave-projections/${debounced.venue}/${debounced.symbol}/${debounced.timeframe}`,
      ),
    refetchInterval: 30_000,
  });

  // ─── Merge pages ────────────────────────────────────────────────
  const merged = useMemo<ChartWorkspace | undefined>(() => {
    if (!query.data) return undefined;
    if (olderPages.length === 0) return query.data;
    const seen = new Set<string>();
    const detections: DetectionOverlay[] = [];
    for (const page of [...olderPages, query.data]) {
      for (const d of page.detections) {
        if (seen.has(d.id)) continue;
        seen.add(d.id);
        detections.push(d);
      }
    }
    const candles: CandleBar[] = [];
    const seenT = new Set<string>();
    for (const page of [...olderPages, query.data]) {
      for (const c of page.candles) {
        if (seenT.has(c.open_time)) continue;
        seenT.add(c.open_time);
        candles.push(c);
      }
    }
    candles.sort((a, b) => a.open_time.localeCompare(b.open_time));
    return { ...query.data, candles, detections };
  }, [query.data, olderPages]);

  const MAX_DETECTIONS_PER_FAMILY = 5;
  const visibleDetections = useMemo(() => {
    if (!merged || !merged.candles?.length) return [];
    // Only show detections whose anchors overlap with visible candle range
    const firstTime = merged.candles[0].open_time;
    const lastTime = merged.candles[merged.candles.length - 1].open_time;
    const filtered = merged.detections.filter((d) => {
      if ((familyModes[d.family] ?? "on") === "off") return false;
      if (!d.anchors?.length) return false;
      // Hide invalidated detections unless in detail mode
      if (d.state === "invalidated" && (familyModes[d.family] ?? "on") !== "detail") return false;
      // At least one anchor must be within the candle range
      const lastAnchor = d.anchors[d.anchors.length - 1]?.time;
      const firstAnchor = d.anchors[0]?.time;
      return lastAnchor >= firstTime && firstAnchor <= lastTime;
    });
    // Sort by score descending
    const sorted = [...filtered].sort((a, b) => (b.score ?? 0) - (a.score ?? 0));

    // Remove overlapping detections within same family: if two detections
    // share >50% of their time range, keep only the higher-scoring one.
    // This eliminates degree mixing (e.g. impulse + diagonal on same swings).
    const kept: typeof sorted = [];
    for (const d of sorted) {
      const dStart = new Date(d.anchors[0].time).getTime();
      const dEnd = new Date(d.anchors[d.anchors.length - 1].time).getTime();
      const dSpan = Math.max(dEnd - dStart, 1);
      const dominated = kept.some((k) => {
        if (k.family !== d.family) return false;
        const kStart = new Date(k.anchors[0].time).getTime();
        const kEnd = new Date(k.anchors[k.anchors.length - 1].time).getTime();
        const kSpan = Math.max(kEnd - kStart, 1);
        const overlapStart = Math.max(dStart, kStart);
        const overlapEnd = Math.min(dEnd, kEnd);
        if (overlapEnd <= overlapStart) return false;
        const overlap = overlapEnd - overlapStart;
        // Dominated if overlap covers >50% of EITHER detection's span
        return overlap / dSpan > 0.5 || overlap / kSpan > 0.5;
      });
      if (!dominated) kept.push(d);
    }

    // Limit per family to avoid clutter
    const countByFamily: Record<string, number> = {};
    return kept.filter((d) => {
      countByFamily[d.family] = (countByFamily[d.family] ?? 0) + 1;
      return countByFamily[d.family] <= MAX_DETECTIONS_PER_FAMILY;
    });
  }, [merged, familyModes]);

  // ─── Prefetch older history ─────────────────────────────────────
  const fetchOlder = useCallback(async () => {
    if (fetchingOlderRef.current || !merged || merged.candles.length === 0) return;
    fetchingOlderRef.current = true;
    try {
      const oldest = merged.candles[0].open_time;
      const page = await apiFetch<ChartWorkspace>(
        `/v2/chart/${debounced.venue}/${debounced.symbol}/${debounced.timeframe}?limit=${PAGE_SIZE}&before=${encodeURIComponent(oldest)}`,
      );
      if (page.candles.length > 0) {
        setOlderPages((prev) => [page, ...prev]);
      }
    } catch (err) {
      console.error("pan-left fetch failed", err);
    } finally {
      fetchingOlderRef.current = false;
    }
  }, [merged, debounced]);

  // ─── Create chart ───────────────────────────────────────────────
  useEffect(() => {
    if (!chartContainerRef.current) return;

    const chartOptions: DeepPartial<ChartOptions> = {
      layout: {
        background: { type: ColorType.Solid, color: "#09090b" },
        textColor: "#71717a",
        fontSize: 11,
        fontFamily: "ui-monospace, SFMono-Regular, monospace",
      },
      grid: {
        vertLines: { color: "#27272a44" },
        horzLines: { color: "#27272a44" },
      },
      crosshair: {
        mode: CrosshairMode.Normal,
        vertLine: { color: "#52525b", style: LineStyle.Dashed, width: 1, labelBackgroundColor: "#27272a" },
        horzLine: { color: "#52525b", style: LineStyle.Dashed, width: 1, labelBackgroundColor: "#27272a" },
      },
      rightPriceScale: {
        borderColor: "#27272a",
        scaleMargins: { top: 0.08, bottom: 0.15 },
        autoScale: true,
      },
      timeScale: {
        borderColor: "#27272a",
        timeVisible: true,
        secondsVisible: false,
        rightOffset: 12,
        barSpacing: 8,
        minBarSpacing: 2,
      },
      handleScroll: { vertTouchDrag: true },
      handleScale: { axisPressedMouseMove: true },
    };

    const chart = createChart(chartContainerRef.current, chartOptions);
    chartRef.current = chart;

    // Candlestick series
    const candleSeries = chart.addSeries(CandlestickSeries, {
      upColor: "#34d399",
      downColor: "#f87171",
      borderUpColor: "#34d399",
      borderDownColor: "#f87171",
      wickUpColor: "#34d39999",
      wickDownColor: "#f8717199",
    });
    candleSeriesRef.current = candleSeries;

    // Volume histogram (separate price scale)
    const volumeSeries = chart.addSeries(HistogramSeries, {
      priceFormat: { type: "volume" },
      priceScaleId: "volume",
    });
    volumeSeriesRef.current = volumeSeries;

    chart.priceScale("volume").applyOptions({
      scaleMargins: { top: 0.85, bottom: 0 },
      visible: false,
    });

    // Responsive resize
    const ro = new ResizeObserver((entries) => {
      if (!chartRef.current) return; // chart disposed
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        try { chart.applyOptions({ width, height }); } catch (_) { /* disposed */ }
      }
    });
    ro.observe(chartContainerRef.current);
    resizeObserverRef.current = ro;

    // Subscribe to visible range changes for prefetch
    chart.timeScale().subscribeVisibleLogicalRangeChange((range) => {
      if (range && range.from < PREFETCH_THRESHOLD) {
        fetchOlder();
      }
    });

    return () => {
      // Don't manually detach/removeSeries — chart.remove() cleans everything.
      markersRef.current = null;
      overlayLinesRef.current = [];
      ro.disconnect();
      try { chart.remove(); } catch (_) { /* already disposed */ }
      chartRef.current = null;
      candleSeriesRef.current = null;
      volumeSeriesRef.current = null;
    };
  }, [debounced.venue, debounced.symbol, debounced.timeframe]);

  // ─── Update data ────────────────────────────────────────────────
  useEffect(() => {
    if (!merged || !candleSeriesRef.current || !volumeSeriesRef.current || !chartRef.current) return;

    const chart = chartRef.current;
    const candleSeries = candleSeriesRef.current;
    const volumeSeries = volumeSeriesRef.current;

    // Convert candles — sort + deduplicate (TV requires strictly ascending time)
    const sorted = [...(merged.candles || [])].filter(c => c && c.open_time).sort(
      (a, b) => new Date(a.open_time).getTime() - new Date(b.open_time).getTime(),
    );
    const candleData: CandlestickData<Time>[] = [];
    const volData: HistogramData<Time>[] = [];
    let prevTs = -1;
    for (const c of sorted) {
      const t = isoToUnix(c.open_time) as number;
      if (t <= prevTs) continue; // skip duplicate or out-of-order
      prevTs = t;
      candleData.push({
        time: t as Time,
        open: Number(c.open),
        high: Number(c.high),
        low: Number(c.low),
        close: Number(c.close),
      });
      volData.push({
        time: t as Time,
        value: Number(c.volume),
        color: Number(c.close) >= Number(c.open) ? "#34d39940" : "#f8717140",
      });
    }

    try {
      candleSeries.setData(candleData);
      volumeSeries.setData(showVolume ? volData : []);
    } catch (_) { return; /* chart disposed between render cycles */ }

    // ── Remove old overlay lines ──
    for (const line of overlayLinesRef.current) {
      try { chart.removeSeries(line); } catch (_) { /* chart disposed */ }
    }
    overlayLinesRef.current = [];
    // ── Remove old rectangle primitives ──
    for (const prim of rectanglePrimitivesRef.current) {
      try { candleSeries.detachPrimitive(prim); } catch (_) { /* disposed */ }
    }
    rectanglePrimitivesRef.current = [];

    // Collect all markers (labels, TP/SL, zigzag) then apply once at the end.
    const allMarkers: SeriesMarker<Time>[] = [];

    // ── Detection overlays ──
    // For each detection, draw line series for the formation + projections
    for (const d of visibleDetections) {
      const color = familyColor(d.family, d.subkind, debounced.timeframe);
      const isZone = ZONE_BOX_SUBKINDS.has(d.subkind);
      const isDashed = d.state === "forming";
      const isInvalidated = d.state === "invalidated";
      const isHov = hoveredRef.current === d.id;
      const isDetailMode = (familyModes[d.family] ?? "on") === "detail";
      const layers = detailLayers[d.family] ?? new Set(["entry_tp_sl", "labels"]);
      // In detail mode, only show projections/TP/SL for the top-scoring detection per family
      const isTopInFamily = (() => {
        const sameFamily = visibleDetections.filter(x => x.family === d.family);
        return sameFamily.length === 0 || sameFamily[0].id === d.id;
      })();
      const showDetail = (isDetailMode && isTopInFamily) || isHov;

      if (d.anchors.length >= 2 && !isZone) {
        // Main formation polyline — solid, thick
        const formLine = chart.addSeries(LineSeries, {
          color: color,
          lineWidth: isInvalidated ? 1 : 3,
          lineStyle: isInvalidated ? LineStyle.Dotted : LineStyle.Solid,
          crosshairMarkerVisible: false,
          lastValueVisible: false,
          priceLineVisible: false,
          pointMarkersVisible: true,
          pointMarkersRadius: 3,
        });

        const lineData = d.anchors.map((a) => ({
          time: isoToUnix(a.time),
          value: Number(a.price),
        }));
        formLine.setData(dedupeLineData(lineData));
        overlayLinesRef.current.push(formLine);

        // Projected anchors — only when measured_move layer enabled
        if (showDetail && layers.has("measured_move") && d.projected_anchors && d.projected_anchors.length > 0) {
          // Projection — dotted, medium width
          const projLine = chart.addSeries(LineSeries, {
            color: color,
            lineWidth: 2,
            lineStyle: LineStyle.Dotted,
            crosshairMarkerVisible: false,
            lastValueVisible: false,
            priceLineVisible: false,
            pointMarkersVisible: true,
            pointMarkersRadius: 2,
          });
          const lastAnchor = d.anchors[d.anchors.length - 1];
          const projData = [
            { time: isoToUnix(lastAnchor.time), value: Number(lastAnchor.price) },
            ...d.projected_anchors.map((a) => ({
              time: isoToUnix(a.time),
              value: Number(a.price),
            })),
          ];
          projLine.setData(dedupeLineData(projData));
          overlayLinesRef.current.push(projLine);
        }

        // Sub-wave decomposition — only when fib_levels layer enabled
        if (showDetail && layers.has("fib_levels") && d.sub_wave_anchors) {
          for (const seg of d.sub_wave_anchors) {
            if (seg.length < 2) continue;
            // Sub-wave — solid, thin, distinct color
            const swLine = chart.addSeries(LineSeries, {
              color: "#93c5fd",
              lineWidth: 1,
              lineStyle: LineStyle.Solid,
              crosshairMarkerVisible: false,
              lastValueVisible: false,
              priceLineVisible: false,
              pointMarkersVisible: true,
              pointMarkersRadius: 2,
            });
            swLine.setData(dedupeLineData(seg.map((a) => ({
              time: isoToUnix(a.time),
              value: Number(a.price),
            }))));
            overlayLinesRef.current.push(swLine);
          }
        }
      }

      // Zone box detections: render as two horizontal price lines
      if (isZone && d.anchors.length >= 2) {
        const p1 = Number(d.anchors[0].price);
        const p2 = Number(d.anchors[1].price);
        const top = Math.max(p1, p2);
        const bot = Math.min(p1, p2);
        for (const price of [top, bot]) {
          const hl = chart.addSeries(LineSeries, {
            color: color,
            lineWidth: 1,
            lineStyle: LineStyle.Dotted,
            crosshairMarkerVisible: false,
            lastValueVisible: false,
            priceLineVisible: false,
          });
          // Extend across the last N bars
          const endTime = isoToUnix(d.anchors[d.anchors.length - 1].time);
          const startIdx = Math.max(0, merged.candles.length - 30);
          const startCandle = merged.candles[startIdx];
          if (!startCandle?.open_time) continue;
          const startTime = isoToUnix(startCandle.open_time);
          hl.setData(sortLineData([
            { time: startTime, value: price },
            { time: endTime, value: price },
          ]));
          overlayLinesRef.current.push(hl);
        }
      }

      // Entry / TP / SL price lines (only when layer enabled)
      if (showDetail && layers.has("entry_tp_sl") && d.anchors.length > 0) {
        const { entry, tp1, tp2, sl } = computeTargets(d);
        const conf = Number(d.confidence) || 0;
        const confStr = conf > 0 ? ` (${(conf * 100).toFixed(0)}%)` : "";
        const lastTime = isoToUnix(d.anchors[d.anchors.length - 1].time);
        const barInterval = merged.candles.length >= 2
          ? (new Date(merged.candles[merged.candles.length - 1].open_time).getTime() -
             new Date(merged.candles[merged.candles.length - 2].open_time).getTime()) / 1000
          : 3600;
        const futureTime = (lastTime as number + barInterval * 20) as Time;
        // midpoint time for label positioning
        const midTime = (lastTime as number + barInterval * 10) as Time;

        const drawLevel = (price: number | null, col: string, style: LineStyle, label: string) => {
          if (!price || !Number.isFinite(price)) return;
          const lvl = chart.addSeries(LineSeries, {
            color: col,
            lineWidth: 1,
            lineStyle: style,
            crosshairMarkerVisible: false,
            // Show label on the price scale (right axis) at the correct price
            lastValueVisible: true,
            priceLineVisible: false,
            title: label,
          });
          lvl.setData(sortLineData([
            { time: lastTime, value: price },
            { time: futureTime, value: price },
          ]));
          overlayLinesRef.current.push(lvl);
        };

        drawLevel(sl, "#ef4444", LineStyle.Dashed, "SL");
        drawLevel(entry, "#d4d4d8", LineStyle.Dotted, `Entry${confStr}`);
        drawLevel(tp1, "#34d399", LineStyle.Dashed, "TP1");
        drawLevel(tp2, "#22c55e80", LineStyle.Dotted, "TP2");
      }
    }

    // ── Projection overlays — draw from source wave endpoint ──
    if (showProjections && projectionsQuery.data && merged.candles.length > 0) {
      const lastCandleT = isoToUnix(merged.candles[merged.candles.length - 1].open_time) as number;

      // Show only rank=1 (leading) per alt_group, not eliminated,
      // whose last leg extends beyond the last visible candle,
      // and whose prices stay within ±60% of current price
      const currentPrice = Number(merged.candles[merged.candles.length - 1].close);
      const projs = projectionsQuery.data
        .filter((p) => {
          if (p.state === "eliminated" || p.rank !== 1) return false;
          const legs: ProjLeg[] = Array.isArray(p.projected_legs) ? p.projected_legs : [];
          const lastLeg = legs[legs.length - 1];
          if (!lastLeg?.time_end_est) return false;
          if ((isoToUnix(lastLeg.time_end_est) as number) <= lastCandleT) return false;
          return true;
        })
        // Sort by max deviation of future-visible points from current price
        .sort((a, b) => {
          const futDev = (legs: ProjLeg[]) => {
            const fp = legs
              .filter((l) => l.time_end_est && (isoToUnix(l.time_end_est) as number) > lastCandleT)
              .flatMap((l) => [l.price_start, l.price_end]);
            if (fp.length === 0) return 999;
            return fp.reduce((mx, pr) => Math.max(mx, Math.abs(pr - currentPrice) / currentPrice), 0);
          };
          return futDev(Array.isArray(a.projected_legs) ? a.projected_legs : [])
               - futDev(Array.isArray(b.projected_legs) ? b.projected_legs : []);
        })
        .slice(0, 2);

      const projColor = (prob: number, dir: string) => {
        if (dir === "bullish") return prob >= 0.4 ? "#a78bfa" : "#7c3aed80";
        return prob >= 0.4 ? "#c084fc" : "#9333ea80";
      };

      for (const proj of projs) {
        const legs: ProjLeg[] = Array.isArray(proj.projected_legs)
          ? proj.projected_legs
          : [];
        if (legs.length === 0) continue;

        // Build full polyline first, then clip to future-only
        const allPts: { time: number; value: number }[] = [];
        for (const leg of legs) {
          if (allPts.length === 0 && leg.time_start_est) {
            allPts.push({ time: isoToUnix(leg.time_start_est) as number, value: leg.price_start });
          }
          if (leg.time_end_est) {
            allPts.push({ time: isoToUnix(leg.time_end_est) as number, value: leg.price_end });
          }
        }
        if (allPts.length < 2) continue;

        // Clip: only keep points after lastCandleT.
        // If a segment crosses lastCandleT, interpolate the crossing price.
        const points: { time: Time; value: number }[] = [];
        for (let i = 0; i < allPts.length; i++) {
          const pt = allPts[i];
          if (pt.time > lastCandleT) {
            // If this is the first future point, interpolate from previous
            if (points.length === 0 && i > 0) {
              const prev = allPts[i - 1];
              const frac = (lastCandleT - prev.time) / (pt.time - prev.time);
              const interpPrice = prev.value + frac * (pt.value - prev.value);
              points.push({ time: lastCandleT as Time, value: interpPrice });
            }
            points.push({ time: pt.time as Time, value: pt.value });
          }
        }
        // If all points are in the future, use them all
        if (points.length === 0 && allPts.length > 0 && allPts[0].time > lastCandleT) {
          for (const pt of allPts) {
            points.push({ time: pt.time as Time, value: pt.value });
          }
        }
        if (points.length < 2) continue;

        const color = projColor(proj.probability, proj.direction);
        const projLine = chart.addSeries(LineSeries, {
          color,
          lineWidth: 2,
          lineStyle: LineStyle.Dotted,
          crosshairMarkerVisible: false,
          lastValueVisible: false,
          priceLineVisible: false,
          pointMarkersVisible: true,
          pointMarkersRadius: 3,
          priceScaleId: "", // overlay — don't affect main Y axis scale
        });
        projLine.setData(dedupeLineData(points));
        overlayLinesRef.current.push(projLine);

        // Invalidation level — red dashed horizontal
        if (proj.invalidation_price) {
          const invPrice = Number(proj.invalidation_price);
          if (Number.isFinite(invPrice)) {
            const invLine = chart.addSeries(LineSeries, {
              color: "#ef444480",
              lineWidth: 1,
              lineStyle: LineStyle.Dashed,
              crosshairMarkerVisible: false,
              lastValueVisible: false,
              priceLineVisible: false,
              priceScaleId: "",
            });
            const t0 = points[0].time as number;
            const t1 = points[points.length - 1].time as number;
            invLine.setData([
              { time: t0 as Time, value: invPrice },
              { time: t1 as Time, value: invPrice },
            ]);
            overlayLinesRef.current.push(invLine);
          }
        }
      }
    }

    // ── Zigzag overlay ──
    if (showZigzag && merged.candles.length > 2) {
      const pts = computeZigzag(merged.candles, ZIGZAG_PCT);
      if (pts.length >= 2) {
        const zigLine = chart.addSeries(LineSeries, {
          color: "#facc15",
          lineWidth: 1,
          lineStyle: LineStyle.Solid,
          crosshairMarkerVisible: false,
          lastValueVisible: false,
          priceLineVisible: false,
          pointMarkersVisible: true,
          pointMarkersRadius: 3,
        });
        zigLine.setData(dedupeLineData(pts.map((p) => ({ time: p.time, value: p.price }))));
        overlayLinesRef.current.push(zigLine);
      }
    }

    // ── Markers: detection anchor labels + zigzag swing labels ──

    // Detection anchor labels (e.g. "1", "2", "3", "W-A", "W-B", etc.)
    if (showLabels) {
      for (const d of visibleDetections) {
        if (ZONE_BOX_SUBKINDS.has(d.subkind)) continue;
        // In detail mode, respect the "labels" layer toggle
        const dLayers = detailLayers[d.family] ?? new Set(["entry_tp_sl", "labels"]);
        const isDetailMode = (familyModes[d.family] ?? "on") === "detail";
        if (isDetailMode && !dLayers.has("labels")) continue;
        const color = familyColor(d.family, d.subkind, debounced.timeframe);
        for (const a of d.anchors) {
          if (!a.label) continue;
          const price = Number(a.price);
          // Determine if anchor is a local high or low relative to neighbors
          const anchorIdx = d.anchors.indexOf(a);
          const prevPrice = anchorIdx > 0 ? Number(d.anchors[anchorIdx - 1].price) : price;
          const nextPrice = anchorIdx < d.anchors.length - 1 ? Number(d.anchors[anchorIdx + 1].price) : price;
          const isTop = price >= prevPrice && price >= nextPrice;
          allMarkers.push({
            time: isoToUnix(a.time),
            position: isTop ? "aboveBar" as const : "belowBar" as const,
            color,
            shape: "circle" as const,
            text: d.family === "elliott"
              ? elliottLabel(a.label, d.subkind, debounced.timeframe)
              : a.label,
          });
        }
        // Subkind + confidence label at last anchor
        const lastAnchor = d.anchors[d.anchors.length - 1];
        if (lastAnchor) {
          const conf = Number(d.confidence) || 0;
          const confPct = conf > 0 ? ` ${(conf * 100).toFixed(0)}%` : "";
          allMarkers.push({
            time: isoToUnix(lastAnchor.time),
            position: "aboveBar" as const,
            color,
            shape: "square" as const,
            text: `${d.has_children ? "＋ " : ""}${d.subkind}${confPct}`,
          });
        }
      }
    }

    // Zigzag swing labels
    if (showZigzag && merged.candles.length > 2) {
      const pts = computeZigzag(merged.candles, ZIGZAG_PCT);
      const swingColors: Record<string, string> = {
        HH: "#22c55e", HL: "#4ade80", LH: "#ef4444", LL: "#f87171",
      };
      for (const p of pts) {
        if (!p.swing) continue;
        allMarkers.push({
          time: p.time,
          position: p.kind === "H" ? "aboveBar" as const : "belowBar" as const,
          color: swingColors[p.swing] ?? "#facc15",
          shape: "circle" as const,
          text: p.swing,
        });
      }
    }

    // Projection leg labels — only for future legs (markers can't go beyond last candle,
    // so we skip projection markers; labels are shown on the line series via pointMarkers)

    // ── Wyckoff event markers ──
    const wyckOverlay = wyckoffQuery.data?.overlay ?? null;
    if (wyckOverlay && (familyModes["wyckoff"] ?? "on") !== "off" && merged.candles?.length) {
      const wEvts = wyckOverlay.events ?? [];
      const eventMeta: Record<string, { label: string; color: string; pos: "aboveBar" | "belowBar" }> = {
        p_s: { label: "PS", color: "#f97316", pos: "belowBar" },
        s_c: { label: "SC", color: "#ef4444", pos: "belowBar" },
        b_c: { label: "BC", color: "#ef4444", pos: "aboveBar" },
        a_r: { label: "AR", color: "#22c55e", pos: "aboveBar" },
        s_t: { label: "ST", color: "#f59e0b", pos: "belowBar" },
        st_b: { label: "STB", color: "#f59e0b", pos: "belowBar" },
        spring: { label: "Spring", color: "#10b981", pos: "belowBar" },
        u_a: { label: "UA", color: "#8b5cf6", pos: "aboveBar" },
        utad: { label: "UTAD", color: "#ef4444", pos: "aboveBar" },
        shakeout: { label: "Shake", color: "#ef4444", pos: "belowBar" },
        s_o_s: { label: "SOS", color: "#22c55e", pos: "aboveBar" },
        s_o_w: { label: "SOW", color: "#ef4444", pos: "belowBar" },
        l_p_s: { label: "LPS", color: "#10b981", pos: "belowBar" },
        lpsy: { label: "LPSY", color: "#ef4444", pos: "belowBar" },
        j_a_c: { label: "JAC", color: "#22c55e", pos: "aboveBar" },
        break_of_ice: { label: "BoI", color: "#ef4444", pos: "belowBar" },
        buec: { label: "BUEC", color: "#3b82f6", pos: "belowBar" },
        s_o_t: { label: "SOT", color: "#6366f1", pos: "aboveBar" },
        markup: { label: "MU", color: "#22c55e", pos: "aboveBar" },
        markdown: { label: "MD", color: "#ef4444", pos: "belowBar" },
      };
      const seen = new Set<string>();
      for (const ev of wEvts) {
        const idx = ev.bar_index;
        if (idx < 0 || idx >= merged.candles.length) continue;
        const key = `${ev.event}_${idx}`;
        if (seen.has(key)) continue;
        seen.add(key);
        const candleTime = isoToUnix(merged.candles[idx].open_time);
        const meta = eventMeta[ev.event] ?? { label: ev.event.toUpperCase(), color: "#9ca3af", pos: "aboveBar" as const };
        allMarkers.push({
          time: candleTime,
          position: meta.pos,
          color: meta.color,
          shape: meta.pos === "aboveBar" ? "arrowDown" : "arrowUp",
          text: meta.label,
        });
      }
    }

    // Sort markers by time (TV requires ascending) and apply
    allMarkers.sort((a, b) => (a.time as number) - (b.time as number));
    if (markersRef.current) {
      try { markersRef.current.detach(); } catch (_) { /* disposed */ }
      markersRef.current = null;
    }
    if (allMarkers.length > 0) {
      markersRef.current = createSeriesMarkers(candleSeries, allMarkers);
    }

    // ── Wyckoff overlay ──
    const wyckoffOverlay = wyckoffQuery.data?.overlay ?? null;
    if (wyckoffOverlay && (familyModes["wyckoff"] ?? "on") !== "off" && merged.candles?.length) {
      const { range: wRange, creek, ice, events: wEvents } = wyckoffOverlay;
      const isAccum = wyckoffOverlay.schematic === "accumulation" || wyckoffOverlay.schematic === "reaccumulation";
      const wFillColor = isAccum ? "#22c55e15" : "#ef444415";
      const wBorderColor = isAccum ? "#22c55e90" : "#ef444490";
      const lastT = isoToUnix(merged.candles[merged.candles.length - 1].open_time);

      // Box starts at structure started_at (like PineScript boxStartBar)
      const structStartT = wyckoffOverlay.started_at
        ? isoToUnix(wyckoffOverlay.started_at)
        : isoToUnix(merged.candles[0].open_time);

      // Helper to attach rectangle primitive
      const addRect = (opts: RectangleOptions) => {
        const prim = new RectanglePrimitive(opts);
        candleSeries.attachPrimitive(prim);
        rectanglePrimitivesRef.current.push(prim);
      };

      // ── Accumulation / Distribution range box ──
      // Like PineScript: box.new(boxStartBar, boxHigh, boxEndBar, boxLow)
      if (wRange.top != null && wRange.bottom != null) {
        const schematicLabel = isAccum ? "Accumulation" : "Distribution";
        const phaseLabel = wyckoffOverlay.phase ? ` — Phase ${wyckoffOverlay.phase}` : "";
        const confLabel = wyckoffOverlay.confidence
          ? ` (${(wyckoffOverlay.confidence * 100).toFixed(0)}%)`
          : "";

        addRect({
          time1: structStartT,
          time2: lastT,
          priceTop: wRange.top,
          priceBottom: wRange.bottom,
          fillColor: wFillColor,
          borderColor: wBorderColor,
          borderWidth: 1,
          label: `${schematicLabel}${phaseLabel}${confLabel}`,
          labelColor: isAccum ? "#22c55ecc" : "#ef4444cc",
          labelSize: 11,
        });
      }
      // ── Creek line (resistance within range) ──
      if (creek != null) {
        const creekLine = chart.addSeries(LineSeries, {
          color: "#3b82f680",
          lineWidth: 1,
          lineStyle: LineStyle.Dashed,
          crosshairMarkerVisible: false,
          lastValueVisible: false,
          priceLineVisible: false,
        });
        creekLine.setData(sortLineData([
          { time: structStartT, value: creek },
          { time: lastT, value: creek },
        ]));
        overlayLinesRef.current.push(creekLine);
      }
      // ── Ice line (support within range) ──
      if (ice != null) {
        const iceLine = chart.addSeries(LineSeries, {
          color: "#ef444480",
          lineWidth: 1,
          lineStyle: LineStyle.Dashed,
          crosshairMarkerVisible: false,
          lastValueVisible: false,
          priceLineVisible: false,
        });
        iceLine.setData(sortLineData([
          { time: structStartT, value: ice },
          { time: lastT, value: ice },
        ]));
        overlayLinesRef.current.push(iceLine);
      }

      // Wyckoff event markers are added to allMarkers above (before marker sort)

      // ── Wyckoff Entry / SL / TP levels ──
      if (wRange.top != null && wRange.bottom != null) {
        const rangeH = wRange.top - wRange.bottom;
        const drawLevel = (price: number, color: string, style: number, label: string) => {
          const lvl = chart.addSeries(LineSeries, {
            color,
            lineWidth: 1,
            lineStyle: style,
            crosshairMarkerVisible: false,
            lastValueVisible: true,
            priceLineVisible: false,
            title: label,
          });
          lvl.setData(sortLineData([
            { time: structStartT, value: price },
            { time: lastT, value: price },
          ]));
          overlayLinesRef.current.push(lvl);
        };

        if (isAccum) {
          // Accumulation: buy setup
          // Entry: just above range bottom (Spring area) or at ice level
          const entry = ice ?? (wRange.bottom + rangeH * 0.15);
          const sl = wRange.bottom - rangeH * 0.05;
          const tp1 = wRange.top;
          const tp2 = wRange.top + rangeH * 0.618;
          const tp3 = wRange.top + rangeH;

          drawLevel(entry, "#3b82f6", LineStyle.Dotted, "Entry");
          drawLevel(sl, "#ef4444", LineStyle.Dotted, "SL");
          drawLevel(tp1, "#22c55e", LineStyle.Dotted, "TP1");
          drawLevel(tp2, "#22c55e80", LineStyle.Dotted, "TP2");
          drawLevel(tp3, "#22c55e60", LineStyle.Dotted, "TP3");
        } else {
          // Distribution: sell setup
          const entry = creek ?? (wRange.top - rangeH * 0.15);
          const sl = wRange.top + rangeH * 0.05;
          const tp1 = wRange.bottom;
          const tp2 = wRange.bottom - rangeH * 0.618;
          const tp3 = wRange.bottom - rangeH;

          drawLevel(entry, "#3b82f6", LineStyle.Dotted, "Entry");
          drawLevel(sl, "#ef4444", LineStyle.Dotted, "SL");
          drawLevel(tp1, "#22c55e", LineStyle.Dotted, "TP1");
          drawLevel(tp2, "#22c55e80", LineStyle.Dotted, "TP2");
          drawLevel(tp3, "#22c55e60", LineStyle.Dotted, "TP3");
        }
      }

      // ── FVG + IFVG detection & visualization ──
      // FVG = 3-candle gap. IFVG = FVG violated by price → bias flips.
      // Minimum gap size filter: ignore tiny gaps (noise)
      if (merged.candles.length >= 3) {
        interface FvgZone {
          c1Idx: number;     // candle 1 index (box starts here)
          c3Idx: number;     // candle 3 index
          top: number;       // exact mum wicks (touches candle)
          bottom: number;
          bull: boolean;
          inverted: boolean;
          violatedAt: number;
          filled: boolean;   // fully filled by price action
        }
        const fvgs: FvgZone[] = [];
        const startIdx = Math.max(0, merged.candles.length - 200);
        const currentPrice = Number(merged.candles[merged.candles.length - 1].close);
        const minGap = currentPrice * 0.0005; // min 0.05% gap to filter noise

        // Step 1: Detect all FVGs
        for (let i = startIdx + 2; i < merged.candles.length; i++) {
          const h1 = Number(merged.candles[i - 2].high);
          const l1 = Number(merged.candles[i - 2].low);
          const h3 = Number(merged.candles[i].high);
          const l3 = Number(merged.candles[i].low);

          // Bullish FVG: C3 low > C1 high (gap up — price jumped)
          if (l3 > h1 && (l3 - h1) > minGap) {
            fvgs.push({ c1Idx: i - 2, c3Idx: i, top: l3, bottom: h1, bull: true, inverted: false, violatedAt: -1, filled: false });
          }
          // Bearish FVG: C1 low > C3 high (gap down — price dropped)
          if (l1 > h3 && (l1 - h3) > minGap) {
            fvgs.push({ c1Idx: i - 2, c3Idx: i, top: l1, bottom: h3, bull: false, inverted: false, violatedAt: -1, filled: false });
          }
        }

        // Step 2: Check fill & IFVG status
        for (const fvg of fvgs) {
          for (let j = fvg.c3Idx + 1; j < merged.candles.length; j++) {
            const cLow = Number(merged.candles[j].low);
            const cHigh = Number(merged.candles[j].high);

            // Check if FVG is fully filled (price crossed through entire gap)
            if (fvg.bull && cLow <= fvg.bottom) {
              // Bullish FVG broken downward → Bearish IFVG
              fvg.inverted = true;
              fvg.violatedAt = j;
              break;
            }
            if (!fvg.bull && cHigh >= fvg.top) {
              // Bearish FVG broken upward → Bullish IFVG
              fvg.inverted = true;
              fvg.violatedAt = j;
              break;
            }
            // Partial fill: price touched CE (50%) but didn't break through
            const ce = (fvg.top + fvg.bottom) / 2;
            if (fvg.bull && cLow <= ce) {
              fvg.filled = true; // partially filled at CE
            }
            if (!fvg.bull && cHigh >= ce) {
              fvg.filled = true;
            }
          }
        }

        // Step 3: Draw — only show unfilled FVGs and recent IFVGs (max 8)
        const active = fvgs.filter((f) => !f.filled || f.inverted).slice(-8);
        for (const fvg of active) {
          // Box starts at C1 candle, extends right
          const sT = isoToUnix(merged.candles[fvg.c1Idx].open_time);
          // Extend to violation point or last candle (like TradingView indicators)
          const endIdx = fvg.inverted && fvg.violatedAt > 0
            ? Math.min(fvg.violatedAt, merged.candles.length - 1)
            : merged.candles.length - 1;
          const eT = isoToUnix(merged.candles[endIdx].open_time);

          let fillColor: string;
          let borderColor: string;
          let label: string;
          if (fvg.inverted) {
            const ifvgBull = !fvg.bull;
            fillColor = ifvgBull ? "#f59e0b18" : "#a855f718";
            borderColor = ifvgBull ? "#f59e0b80" : "#a855f780";
            label = ifvgBull ? "IFVG+" : "IFVG-";
          } else {
            fillColor = fvg.bull ? "#3b82f618" : "#ef444418";
            borderColor = fvg.bull ? "#3b82f670" : "#ef444470";
            label = fvg.bull ? "FVG+" : "FVG-";
          }

          addRect({
            time1: sT,
            time2: eT,
            priceTop: fvg.top,
            priceBottom: fvg.bottom,
            fillColor,
            borderColor,
            borderWidth: 1,
            label,
            labelColor: borderColor,
            labelSize: 9,
          });
        }
      }
    }

  }, [merged, showVolume, showZigzag, showLabels, showProjections, visibleDetections, familyModes, detailLayers, wyckoffQuery.data, projectionsQuery.data]);

  // ═══════════════════════════════════════════════════════════════
  // RENDER
  // ═══════════════════════════════════════════════════════════════

  return (
    <div className="flex h-[calc(100vh-3rem)]">
      {/* ── Left Toolbar ────────────────────────────────────────── */}
      <div className="flex w-10 flex-col items-center gap-1 border-r border-zinc-800 bg-zinc-950 py-2">
        {TOOLS.map((tool) => (
          <button
            key={tool.id}
            type="button"
            onClick={() => setActiveTool(tool.id)}
            title={tool.tip}
            className={`flex h-8 w-8 items-center justify-center rounded text-sm transition ${
              activeTool === tool.id
                ? "bg-zinc-700 text-zinc-100"
                : "text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300"
            }`}
          >
            {tool.icon}
          </button>
        ))}
        <div className="my-1 h-px w-6 bg-zinc-800" />
        {/* Volume toggle */}
        <button
          type="button"
          onClick={() => setShowVolume((v) => !v)}
          title={showVolume ? "Volume gizle" : "Volume göster"}
          className={`flex h-8 w-8 items-center justify-center rounded text-[10px] font-bold transition ${
            showVolume
              ? "bg-emerald-500/20 text-emerald-300"
              : "text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300"
          }`}
        >
          V
        </button>
        {/* Zigzag toggle */}
        <button
          type="button"
          onClick={() => setShowZigzag((v) => !v)}
          title={showZigzag ? "Zigzag gizle" : "Zigzag göster"}
          className={`flex h-8 w-8 items-center justify-center rounded text-[10px] font-bold transition ${
            showZigzag
              ? "bg-yellow-500/20 text-yellow-300"
              : "text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300"
          }`}
        >
          Z
        </button>
        {/* Labels toggle */}
        <button
          type="button"
          onClick={() => setShowLabels((v) => !v)}
          title={showLabels ? "Label gizle" : "Label göster"}
          className={`flex h-8 w-8 items-center justify-center rounded text-[10px] font-bold transition ${
            showLabels
              ? "bg-sky-500/20 text-sky-300"
              : "text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300"
          }`}
        >
          Aa
        </button>
        {/* Projections toggle */}
        <button
          type="button"
          onClick={() => setShowProjections((v) => !v)}
          title={showProjections ? "Projeksiyonları gizle" : "Projeksiyonları göster"}
          className={`flex h-8 w-8 items-center justify-center rounded text-[10px] transition ${
            showProjections
              ? "bg-purple-500/20 text-purple-300"
              : "text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300"
          }`}
        >
          🔮
        </button>
      </div>

      {/* ── Main Area ───────────────────────────────────────────── */}
      <div className="flex flex-1 flex-col overflow-hidden">
        {/* ── Top Toolbar (single row, TV-style) ──────────────── */}
        <div className="flex items-center gap-2 border-b border-zinc-800 bg-zinc-950 px-3 py-1.5">
          {/* Symbol */}
          <div className="flex items-center gap-1">
            <input
              value={form.venue}
              onChange={(e) => setForm({ ...form, venue: e.target.value })}
              className="w-20 rounded border border-zinc-700 bg-zinc-900 px-1.5 py-0.5 text-[11px] text-zinc-300"
              placeholder="venue"
            />
            <input
              value={form.symbol}
              onChange={(e) => setForm({ ...form, symbol: e.target.value.toUpperCase() })}
              className="w-24 rounded border border-zinc-700 bg-zinc-900 px-1.5 py-0.5 font-mono text-[11px] font-semibold text-zinc-100"
              placeholder="BTCUSDT"
            />
          </div>

          {/* Separator */}
          <div className="h-5 w-px bg-zinc-700" />

          {/* Timeframes */}
          <div className="flex items-center gap-0.5">
            {TIMEFRAMES.map((tf) => (
              <button
                key={tf}
                type="button"
                onClick={() => setForm({ ...form, timeframe: tf })}
                className={`rounded px-1.5 py-0.5 text-[11px] font-medium transition ${
                  tf === form.timeframe
                    ? "bg-zinc-700 text-zinc-100"
                    : "text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300"
                }`}
              >
                {tf}
              </button>
            ))}
          </div>

          {/* Separator */}
          <div className="h-5 w-px bg-zinc-700" />

          {/* Family toggles (compact) */}
          <div className="flex items-center gap-1">
            {Object.entries(FAMILY_COLORS)
              .filter(([k]) => k !== "custom")
              .map(([family, color]) => {
                const mode = familyModes[family] ?? "on";
                const isOff = mode === "off";
                const isDetail = mode === "detail";
                return (
                  <button
                    key={family}
                    type="button"
                    onClick={() => cycleFamily(family)}
                    onContextMenu={(e) => { e.preventDefault(); detailFamily(family); }}
                    className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] uppercase tracking-wide transition"
                    style={{
                      borderWidth: 1,
                      borderColor: isOff ? "#3f3f46" : color,
                      background: isOff ? "transparent" : isDetail ? `${color}33` : `${color}18`,
                      color: isOff ? "#71717a" : color,
                    }}
                    title={isOff ? "Gizli (sağ tık: detay)" : isDetail ? "Detay (sağ tık: kapat)" : "Görünür (sağ tık: detay)"}
                  >
                    <span
                      className="inline-block h-1.5 w-2.5 rounded-sm"
                      style={{ background: isOff ? "#3f3f46" : color }}
                    />
                    {family}
                  </button>
                );
              })}
          </div>

          {/* Right side: info */}
          <div className="ml-auto flex items-center gap-3 text-[10px] text-zinc-600">
            {merged && (
              <span>
                {merged.candles.length} mum · {visibleDetections.length}/{merged.detections.length} tespit
              </span>
            )}
            <span className="flex items-center gap-2">
              <span className="inline-block h-0 w-3 border-t border-dashed border-zinc-500" /> forming
              <span className="inline-block h-0 w-3 border-t border-zinc-400" /> confirmed
            </span>
          </div>
        </div>

        {/* Detail sub-menu bar (conditional) */}
        {Object.entries(familyModes).some(([, m]) => m === "detail") && (
          <div className="flex items-center gap-1 border-b border-zinc-800/60 bg-zinc-900/80 px-3 py-1">
            {Object.entries(FAMILY_COLORS)
              .filter(([f]) => (familyModes[f] ?? "on") === "detail")
              .map(([family, color]) => {
                const layers = detailLayers[family] ?? new Set(["entry_tp_sl"]);
                return (
                  <div key={family} className="flex items-center gap-1">
                    <span className="text-[9px] font-bold uppercase tracking-widest" style={{ color }}>
                      {family}
                    </span>
                    {DETAIL_SUB_BUTTONS.map((btn) => {
                      const active = layers.has(btn.key);
                      return (
                        <button
                          key={btn.key}
                          type="button"
                          onClick={() => toggleLayer(family, btn.key)}
                          className="rounded border px-1 py-0.5 text-[9px] transition"
                          style={{
                            borderColor: active ? color : "#3f3f46",
                            background: active ? `${color}33` : "transparent",
                            color: active ? color : "#71717a",
                          }}
                        >
                          {btn.icon} {btn.label}
                        </button>
                      );
                    })}
                    <span className="mx-1 h-3 w-px bg-zinc-700" />
                  </div>
                );
              })}
          </div>
        )}

        {/* ── Chart Canvas ────────────────────────────────────── */}
        {query.isLoading && !merged && (
          <div className="flex flex-1 items-center justify-center text-sm text-zinc-400">
            Grafik yükleniyor…
          </div>
        )}
        {query.isError && (
          <div className="m-4 rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
            Hata: {(query.error as Error).message}
          </div>
        )}

        <div
          ref={chartContainerRef}
          className="flex-1 bg-zinc-950"
          style={{ minHeight: 500 }}
        />

        {/* ── Detections Table ─────────────────────────────────── */}
        {merged && merged.detections.length > 0 && (
          <div className="max-h-36 overflow-auto border-t border-zinc-800 bg-zinc-950">
            <table className="w-full text-[11px]">
              <thead className="sticky top-0 bg-zinc-900 text-[10px] uppercase text-zinc-500">
                <tr>
                  <th className="px-2 py-1 text-left">Kind</th>
                  <th className="px-2 py-1 text-left">State</th>
                  <th className="px-2 py-1 text-left">When</th>
                  <th className="px-2 py-1 text-right">Price</th>
                  <th className="px-2 py-1 text-right">Stop</th>
                  <th className="px-2 py-1 text-right">Conf</th>
                </tr>
              </thead>
              <tbody className="font-mono text-zinc-100">
                {merged.detections.map((d) => (
                  <Fragment key={d.id}>
                  <tr
                    className={`border-t border-zinc-800/40 transition-colors ${
                      hovered === d.id ? "bg-zinc-800/60" : "hover:bg-zinc-900"
                    }`}
                    onMouseEnter={() => setHovered(d.id)}
                    onMouseLeave={() => setHovered(null)}
                    onDoubleClick={() => {
                      // Drill-down: double-click Elliott detection → jump to child TF
                      if (d.family === "elliott") {
                        // Dynamic child TF based on wave DURATION, not fixed mapping
                        // Short waves → skip to lower TF, long waves → use next TF
                        const TF_ORDER = ["1M","1w","1d","4h","1h","30m","15m","5m","3m","1m"];
                        const TF_SECONDS: Record<string,number> = {
                          "1M":2592000,"1w":604800,"1d":86400,"4h":14400,
                          "1h":3600,"30m":1800,"15m":900,"5m":300,"3m":180,"1m":60,
                        };
                        const curIdx = TF_ORDER.indexOf(debounced.timeframe);
                        if (curIdx >= 0 && d.anchors.length >= 2) {
                          const waveStart = new Date(d.anchors[0].time).getTime() / 1000;
                          const waveEnd = new Date(d.anchors[d.anchors.length-1].time).getTime() / 1000;
                          const duration = waveEnd - waveStart;
                          // Pick child TF: want ~20-80 bars within the wave duration
                          let bestTf = TF_ORDER[Math.min(curIdx+1, TF_ORDER.length-1)];
                          for (let i = curIdx+1; i < TF_ORDER.length; i++) {
                            const bars = duration / TF_SECONDS[TF_ORDER[i]];
                            if (bars >= 20 && bars <= 200) { bestTf = TF_ORDER[i]; break; }
                            if (bars < 20) { bestTf = TF_ORDER[Math.max(i-1, curIdx+1)]; break; }
                          }
                          setForm((f) => ({ ...f, timeframe: bestTf }));
                          setDebounced((f) => ({ ...f, timeframe: bestTf }));
                        }
                      }
                    }}
                    title={d.family === "elliott" ? "Çift tıkla → alt TF'ye in" : ""}
                  >
                    <td className="px-2 py-0.5" style={{ color: familyColor(d.family, d.subkind, debounced.timeframe) }}>
                      {d.has_children && <span className="text-yellow-400 mr-1 cursor-pointer" title="Alt dalga var — çift tıkla">＋</span>}
                      {d.subkind}
                    </td>
                    <td className="px-2 py-0.5 text-zinc-400">{d.state}</td>
                    <td className="px-2 py-0.5 text-zinc-500">{d.anchor_time}</td>
                    <td className="px-2 py-0.5 text-right">{d.anchor_price}</td>
                    <td className="px-2 py-0.5 text-right text-zinc-500">{d.invalidation_price}</td>
                    <td className="px-2 py-0.5 text-right">{d.confidence}</td>
                  </tr>
                  {d.wave_context && (
                    <tr className="border-t border-zinc-800/20">
                      <td colSpan={6} className="px-2 py-0.5 text-[10px] text-sky-400/70">
                        📐 {d.wave_context}
                      </td>
                    </tr>
                  )}
                  </Fragment>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}

export default Chart;
