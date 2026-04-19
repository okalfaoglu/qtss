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
import { useChartPalette } from "../lib/use-chart-palette";
import { RectanglePrimitive, type RectangleOptions } from "../lib/rectangle-primitive";
import {
  dispatchRenderGeometry,
  type RenderContext,
  type RenderSinks,
} from "../lib/render-kind-registry";
import type { CandleBar, ChartWorkspace, DetectionOverlay } from "../lib/types";

// ─── Constants ───────────────────────────────────────────────────────
const DEFAULTS = { venue: "binance", segment: "futures", symbol: "BTCUSDT", timeframe: "1h" };
const PAGE_SIZE = 500;
const PREFETCH_THRESHOLD = 50;

// Bootstrap defaults — `useChartPalette` overwrites this object when
// the Config Editor response arrives (Aşama 5.C). Kept mutable so every
// existing `familyColor()` caller picks up DB overrides without needing
// to thread palette through the whole component tree.
const FAMILY_COLORS: Record<string, string> = {
  elliott: "#7dd3fc",
  harmonic: "#f472b6",
  classical: "#facc15",
  wyckoff: "#a78bfa",
  range: "#5eead4",
  tbm: "#fb923c",
  candle: "#fca5a5",
  gap: "#38bdf8",
  custom: "#d4d4d8",
};

const STYLE_COLORS: Record<string, string> = {};

// P19 — user-facing display names. The `range` family is actually SMC
// zones (FVG / OB / Liquidity Pool / Equal Levels); "RANGE" was
// misleading (user expected trading-range channels).
const FAMILY_DISPLAY: Record<string, string> = {
  elliott: "elliott",
  harmonic: "harmonic",
  classical: "classical",
  wyckoff: "wyckoff",
  range: "zones",
  tbm: "tbm",
  candle: "mum",
  gap: "boşluk",
  custom: "custom",
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

// P5 — human-readable classical subkind labels for chart markers.
// Falls back to raw subkind when not listed.
const CLASSICAL_SUBKIND_LABELS: Record<string, string> = {
  // existing
  double_top_bear: "Double Top",
  double_bottom_bull: "Double Bottom",
  head_and_shoulders_bear: "H&S",
  inverse_head_and_shoulders_bull: "Inv H&S",
  ascending_triangle_bull: "Asc Triangle",
  descending_triangle_bear: "Desc Triangle",
  symmetrical_triangle_neutral: "Sym Triangle",
  // P5.1
  rectangle_neutral: "Rectangle",
  // P5.2
  bull_flag: "Bull Flag",
  bear_flag: "Bear Flag",
  pennant_bull: "Bull Pennant",
  pennant_bear: "Bear Pennant",
  // P5.3
  rising_wedge_bear: "Rising Wedge",
  falling_wedge_bull: "Falling Wedge",
  // P5.4
  ascending_channel_bull: "Asc Channel",
  descending_channel_bear: "Desc Channel",
  // P5.5
  cup_and_handle_bull: "Cup & Handle",
  inverse_cup_and_handle_bear: "Inv Cup & Handle",
  rounding_bottom_bull: "Rounding Bottom",
  rounding_top_bear: "Rounding Top",
  // P5.6
  diamond_top_bear: "Diamond Top",
  diamond_bottom_bull: "Diamond Bottom",
};

function classicalSubkindLabel(subkind: string): string {
  return CLASSICAL_SUBKIND_LABELS[subkind] ?? subkind.replace(/_/g, " ");
}

// P5 — patterns whose two trendlines (upper / lower) should render as
// SEPARATE polylines instead of a single zigzag through alternating
// pivots. Detected by subkind prefix.
const TWO_TRENDLINE_PREFIXES = [
  "rectangle",
  "ascending_triangle", "descending_triangle", "symmetrical_triangle",
  "rising_wedge", "falling_wedge",
  "ascending_channel", "descending_channel",
  "bull_flag", "bear_flag",
  "pennant",
  "diamond_top", "diamond_bottom",
];
function isTwoTrendlinePattern(subkind: string): boolean {
  return TWO_TRENDLINE_PREFIXES.some((p) => subkind.startsWith(p));
}

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

// Per-family detail-menu buttons. `families` omitted = applies to every
// family; listing families restricts the button so we don't pollute the
// toolbar with toggles that have no renderer for that family (e.g.
// classical detections carry no projected_anchors / sub_wave_anchors,
// so Fib Levels + Measured Move would be inert for CLASSICAL). The
// `invalidation` key was dead (never read by the renderer) and is
// removed entirely.
const DETAIL_SUB_BUTTONS: Array<{
  key: string;
  label: string;
  icon: string;
  families?: string[];
}> = [
  { key: "entry_tp_sl", label: "Entry / TP / SL", icon: "⊞" },
  { key: "labels", label: "Labels", icon: "Aa" },
  // Sub-wave decomposition (Elliott-only in practice — harmonic/wyckoff
  // rows don't emit sub_wave_anchors).
  { key: "fib_levels", label: "Fib Levels", icon: "φ", families: ["elliott", "harmonic"] },
  // Forward projection — emitted by Elliott (projected next wave) and
  // harmonic (PRZ projection).
  { key: "measured_move", label: "Measured Move", icon: "⟷", families: ["elliott", "harmonic"] },
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

function elliottColor(_subkind: string, timeframe: string): string {
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
  events: Array<{ event: string; bar_index: number; price: number; score: number; time_ms?: number | null }>;
  started_at: string;
}

// ─── Chart form state ────────────────────────────────────────────────
interface ChartForm {
  venue: string;
  segment: string;
  symbol: string;
  timeframe: string;
}

// Faz 9.8.x — chart venues list (populates Exchange/Segment/Symbol combos
// from engine_symbols so the frontend never hardcodes names).
interface ChartVenueOption {
  exchange: string;
  segment: string;
  symbols: string[];
  intervals: string[];
}

// ─── Compute Entry/TP/SL from detection geometry ─────────────────────
/** Frontend mirror of `compute_structural_targets_raw`
 * (crates/qtss-worker/src/v2_setup_loop.rs). Kept in sync manually so
 * the chart overlay shows exactly the entry/SL/TP ladder the setup
 * engine would arm — with formation-specific labels ("MM 1.0x",
 * "ABCD 1.272x", "Pat 1.618x"). Any change to the backend formulas
 * must be reflected here; the unit-tested Rust version is the source
 * of truth. Returns an empty list when the formation has no
 * structural geometry (caller falls back to displaying SL only).
 */
const CLASSICAL_HEIGHT_PREFIXES = [
  "rising_wedge", "falling_wedge",
  "bull_flag", "bear_flag", "pennant",
  "ascending_channel", "descending_channel",
  "ascending_triangle", "descending_triangle", "symmetrical_triangle",
  "rectangle",
  "diamond_top", "diamond_bottom",
  "broadening",
  "cup_and_handle", "inverse_cup_and_handle",
  "rounding_top", "rounding_bottom",
  "scallop_bullish", "scallop_bearish",
  "measured_move_abcd",
];

interface StructuralLevel { price: number; label: string }

function directionSign(subkind: string): 0 | 1 | -1 {
  if (subkind.endsWith("_bull") || subkind.includes("_bull_")) return 1;
  if (subkind.endsWith("_bear") || subkind.includes("_bear_")) return -1;
  return 0;
}

function computeFormationTargets(d: DetectionOverlay): {
  entry: number | null;
  sl: number | null;
  targets: StructuralLevel[];
} {
  const inv = Number(d.invalidation_price);
  const sl = Number.isFinite(inv) && inv > 0 ? inv : null;
  const anchors = d.anchors;
  const prices = anchors.map((a) => Number(a.price));
  const sub = d.subkind;
  const sign = directionSign(sub);
  const empty = { entry: null as number | null, sl, targets: [] as StructuralLevel[] };
  if (sign === 0 || anchors.length === 0) return empty;

  // double_top / double_bottom — 3 anchors, project from neckline
  if (anchors.length >= 3 && (sub.startsWith("double_top") || sub.startsWith("double_bottom"))) {
    const extreme = prices[0], neck = prices[1];
    const h = Math.abs(extreme - neck);
    if (h > 0) {
      return {
        entry: neck, sl,
        targets: [
          { price: neck + sign * h,         label: "MM 1.0x" },
          { price: neck + sign * h * 1.618, label: "MM 1.618x" },
        ],
      };
    }
  }

  // head & shoulders — 5 anchors
  if (anchors.length >= 5 && sub.includes("head_and_shoulders")) {
    const head = prices[2], n1 = prices[1], n2 = prices[3];
    const neck = (n1 + n2) / 2;
    const h = Math.abs(head - neck);
    if (h > 0) {
      return {
        entry: neck, sl,
        targets: [
          { price: neck + sign * h,         label: "MM 1.0x" },
          { price: neck + sign * h * 1.618, label: "MM 1.618x" },
        ],
      };
    }
  }

  // triple_top / triple_bottom — 5 anchors
  if (anchors.length >= 5 && (sub.startsWith("triple_top") || sub.startsWith("triple_bottom"))) {
    const [p1, v1, p2, v2, p3] = prices;
    const neck = (v1 + v2) / 2;
    const peak = (p1 + p2 + p3) / 3;
    const h = Math.abs(peak - neck);
    if (h > 0) {
      return {
        entry: neck, sl,
        targets: [
          { price: neck + sign * h,         label: "MM 1.0x" },
          { price: neck + sign * h * 1.618, label: "MM 1.618x" },
        ],
      };
    }
  }

  // measured_move_abcd — 4 anchors, AB=CD from D
  if (anchors.length >= 4 && sub.startsWith("measured_move_abcd")) {
    const a = prices[0], b = prices[1], dPt = prices[3];
    const ab = Math.abs(b - a);
    if (ab > 0) {
      return {
        entry: dPt, sl,
        targets: [
          { price: dPt + sign * ab * 1.000, label: "ABCD 1.0x" },
          { price: dPt + sign * ab * 1.272, label: "ABCD 1.272x" },
          { price: dPt + sign * ab * 1.618, label: "ABCD 1.618x" },
        ],
      };
    }
  }

  // v_top / v_bottom — 3 anchors
  if (anchors.length >= 3 && (sub.startsWith("v_top") || sub.startsWith("v_bottom"))) {
    const tip = prices[1], neck = prices[0];
    const h = Math.abs(tip - neck);
    if (h > 0) {
      return {
        entry: neck, sl,
        targets: [
          { price: neck + sign * h * 0.618, label: "V 0.618x" },
          { price: neck + sign * h * 1.000, label: "V 1.0x" },
        ],
      };
    }
  }

  // Generic classical pattern-height projection — wedges, flags,
  // channels, rectangles, diamonds, broadening, cup & handle, rounding,
  // scallops. Height = range of all anchor prices, projected from
  // *entry* (last anchor = breakout pivot). Projecting from the
  // invalidation edge would place TP1 at ~entry (since h ≈ entry - inv
  // for these patterns) — the label would collide with Entry and the
  // 1.618× extension would sit too close to be useful.
  if (
    d.family === "classical" &&
    CLASSICAL_HEIGHT_PREFIXES.some((p) => sub.startsWith(p))
  ) {
    const valid = prices.filter((x) => Number.isFinite(x));
    if (valid.length >= 2) {
      const h = Math.max(...valid) - Math.min(...valid);
      const entry = prices[prices.length - 1];
      if (h > 0 && entry > 0) {
        return {
          entry, sl,
          targets: [
            { price: entry + sign * h,         label: "Pat 1.0x" },
            { price: entry + sign * h * 1.618, label: "Pat 1.618x" },
          ],
        };
      }
    }
  }

  // Harmonic XABCD — entry at D, project AD retracement
  if (d.family === "harmonic" && anchors.length >= 5) {
    const aP = prices[1];
    const dP = prices[prices.length - 1];
    const ad = Math.abs(aP - dP);
    // Use geometry-derived direction for harmonic (subkind may be neutral)
    const cP = prices[3];
    const hSign = dP < cP ? 1 : -1;
    if (ad > 0) {
      return {
        entry: dP, sl,
        targets: [
          { price: dP + hSign * ad * 0.382, label: "AD 0.382" },
          { price: dP + hSign * ad * 0.618, label: "AD 0.618" },
          { price: dP + hSign * ad * 1.000, label: "AD 1.000" },
        ],
      };
    }
  }

  // Elliott impulse 1-2-3-4-5 — project wave-1 height from wave-4 end
  if (sub.includes("impulse") && anchors.length >= 6) {
    const p0 = prices[0], p1 = prices[1], p4 = prices[4];
    const w1 = Math.abs(p1 - p0);
    if (w1 > 0) {
      return {
        entry: p4, sl,
        targets: [
          { price: p4 + sign * w1,         label: "W1 1.0x" },
          { price: p4 + sign * w1 * 1.618, label: "W1 1.618x" },
        ],
      };
    }
  }

  return empty;
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
  // Backlog items — Setup + Open Position overlays. Default OFF because
  // Chart already carries classical/elliott/harmonic/wyckoff/zones/tbm/
  // mum/boşluk layers; layering armed setups + live positions on top of
  // everything always-on becomes a wall of lines. Toolbar toggles let
  // the operator opt-in per viewing session.
  const [showSetups, setShowSetups] = useState(false);
  const [showPositions, setShowPositions] = useState(false);
  const [activeTool, setActiveTool] = useState<ToolId>("crosshair");

  // Aşama 5.C — palette from system_config (DB-tunable via Config
  // Editor, CLAUDE.md #2). Bootstrap defaults live in FAMILY_COLORS;
  // here we overwrite them in-place so every downstream `familyColor()`
  // call (there are dozens) picks up overrides without prop drilling.
  const palette = useChartPalette();
  useEffect(() => {
    for (const [k, v] of Object.entries(palette.family)) FAMILY_COLORS[k] = v;
    for (const [k, v] of Object.entries(palette.style)) STYLE_COLORS[k] = v;
  }, [palette]);

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

  // Faz 9.8.x — exchange/segment/symbol combobox source. Long cache
  // because engine_symbols changes rarely.
  const venuesQuery = useQuery({
    queryKey: ["v2", "chart", "venues"],
    queryFn: () => apiFetch<ChartVenueOption[]>("/v2/chart/venues"),
    staleTime: 60_000,
  });
  const venueOptions = venuesQuery.data ?? [];

  // ─── Data queries ───────────────────────────────────────────────
  const query = useQuery({
    queryKey: ["v2", "chart", debounced],
    queryFn: () =>
      apiFetch<ChartWorkspace>(
        `/v2/chart/${debounced.venue}/${debounced.symbol}/${debounced.timeframe}?limit=${PAGE_SIZE}&segment=${debounced.segment}`,
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

  // ─── Multi-structure event feed (Faz 10 follow-up) ──────────────
  // /v2/wyckoff/overlay only returns the single ACTIVE structure for
  // the (symbol,TF). When no structure is active the chart had zero
  // Wyckoff labels even though completed/failed structures had rich
  // event data. /v2/wyckoff/events flattens active+recent structures
  // and tags each event with `validate_event_placement` violation
  // info — operator gets:
  //   1) labels even on symbols with no live structure
  //   2) yellow halo over events that broke phase/direction coherence
  interface WyckEventRow {
    event_code: string;
    full_name: string;
    phase: string;
    family: string;
    bar_index: number | null;
    price: number | null;
    score: number;
    time_ms: number | null;
    violation: { kind: string; reason: string } | null;
  }
  const wyckEventsQuery = useQuery({
    enabled: (familyModes["wyckoff"] ?? "on") !== "off",
    queryKey: ["v2", "wyckoff", "events", debounced.symbol, debounced.timeframe],
    queryFn: () =>
      apiFetch<{ events: WyckEventRow[]; violation_count: number }>(
        `/v2/wyckoff/events?symbol=${encodeURIComponent(debounced.symbol)}&interval=${encodeURIComponent(debounced.timeframe)}&limit=300`,
      ),
    refetchInterval: 30_000,
  });

  // ─── Setup overlay feed (Faz 8 /v2/setups) ──────────────────────
  // Backlog item — armed+active setups on the current (symbol,timeframe).
  // API takes only a single `state` param so we fetch all non-closed
  // states by omitting it and filter client-side. Profile / direction
  // drive the line colour; close_reason!=null rows are excluded.
  interface SetupOverlayRow {
    id: string;
    symbol: string;
    timeframe: string;
    profile: string;
    state: string;
    direction: string;
    alt_type: string | null;
    entry_price: number | null;
    entry_sl: number | null;
    koruma: number | null;
    target_ref: number | null;
    close_reason: string | null;
    ai_score: number | null;
    trail_mode: boolean | null;
    raw_meta: { structural_targets?: Array<{ price: number; weight: number; label: string }> } | null;
  }
  const setupsQuery = useQuery({
    enabled: showSetups,
    queryKey: ["v2", "chart-setups", debounced.symbol, debounced.timeframe],
    queryFn: () =>
      apiFetch<{ entries: SetupOverlayRow[] }>(
        `/v2/setups?symbol=${encodeURIComponent(debounced.symbol)}&timeframe=${encodeURIComponent(debounced.timeframe)}&limit=200`,
      ),
    refetchInterval: 30_000,
  });

  // ─── Live-position overlay feed (/v2/live-positions) ────────────
  // API is global (no symbol filter); we filter by symbol client-side.
  // `include_closed=false` so ledger history doesn't crowd the pane —
  // closed positions belong to a separate history panel (out of scope).
  interface LivePositionRow {
    id: string;
    setup_id: string | null;
    mode: string;
    symbol: string;
    side: string;
    leverage: number;
    entry_avg: string;
    qty_filled: string;
    qty_remaining: string;
    current_sl: string | null;
    liquidation_price: string | null;
    last_mark: string | null;
    unrealized_pnl_quote: string | null;
    tp_ladder: unknown;
  }
  const positionsQuery = useQuery({
    enabled: showPositions,
    queryKey: ["v2", "chart-positions", debounced.symbol],
    queryFn: () => apiFetch<LivePositionRow[]>(`/v2/live-positions?include_closed=false&limit=200`),
    refetchInterval: 15_000,
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
    const sorted = [...filtered].sort((a, b) => ((b as any).score ?? 0) - ((a as any).score ?? 0));

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

    // P19d — SMC zones overlap in TIME (all formation→now), so the
    // time-based dedup above doesn't help. Add PRICE-overlap dedup
    // for range/zones family: if two zones share >60% of their price
    // band, keep only the higher-scoring one. Fixes the stacked
    // "EQUAL LOWS + LIQUIDITY POOL LOW" visual clutter.
    const zoneKept: typeof kept = [];
    for (const d of kept) {
      if (d.family !== "range" || d.anchors.length < 2) {
        zoneKept.push(d);
        continue;
      }
      const p1 = Number(d.anchors[0].price);
      const p2 = Number(d.anchors[1].price);
      const dTop = Math.max(p1, p2);
      const dBot = Math.min(p1, p2);
      const dBand = Math.max(dTop - dBot, 1e-9);
      const priceDominated = zoneKept.some((k) => {
        if (k.family !== "range" || k.anchors.length < 2) return false;
        const kp1 = Number(k.anchors[0].price);
        const kp2 = Number(k.anchors[1].price);
        const kTop = Math.max(kp1, kp2);
        const kBot = Math.min(kp1, kp2);
        const kBand = Math.max(kTop - kBot, 1e-9);
        const oTop = Math.min(dTop, kTop);
        const oBot = Math.max(dBot, kBot);
        if (oTop <= oBot) return false;
        const overlap = oTop - oBot;
        return overlap / dBand > 0.6 || overlap / kBand > 0.6;
      });
      if (!priceDominated) zoneKept.push(d);
    }

    // Limit per family to avoid clutter. Zones get a tighter cap
    // than structural detections — even after price dedup, stacking
    // 5 FVG/OB/LP boxes makes labels unreadable.
    const countByFamily: Record<string, number> = {};
    const MAX_PER_FAMILY_ZONE = 3;
    return zoneKept.filter((d) => {
      countByFamily[d.family] = (countByFamily[d.family] ?? 0) + 1;
      const cap = d.family === "range"
        ? MAX_PER_FAMILY_ZONE
        : MAX_DETECTIONS_PER_FAMILY;
      return countByFamily[d.family] <= cap;
    });
  }, [merged, familyModes]);

  // ─── Prefetch older history ─────────────────────────────────────
  const fetchOlder = useCallback(async () => {
    if (fetchingOlderRef.current || !merged || merged.candles.length === 0) return;
    fetchingOlderRef.current = true;
    try {
      const oldest = merged.candles[0].open_time;
      const page = await apiFetch<ChartWorkspace>(
        `/v2/chart/${debounced.venue}/${debounced.symbol}/${debounced.timeframe}?limit=${PAGE_SIZE}&segment=${debounced.segment}&before=${encodeURIComponent(oldest)}`,
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

      // Aşama 5 — opt-in explicit geometry dispatch. When the detector
      // emitted `render_geometry`, route through RENDER_KIND_REGISTRY
      // and skip the legacy anchor path entirely for that detection.
      if (d.render_geometry && d.render_geometry.kind) {
        const regMarkers: SeriesMarker<Time>[] = [];
        const sinks: RenderSinks = {
          rects: rectanglePrimitivesRef.current,
          lines: overlayLinesRef.current,
          markers: regMarkers,
        };
        // Aşama 5.C — style key overrides family color (bull/bear, etc).
        const styleColor = d.render_style ? STYLE_COLORS[d.render_style] : undefined;
        const ctx: RenderContext = {
          chart: chartRef.current!,
          candleSeries,
          sinks,
          isoToUnix,
          familyColor: styleColor ?? color,
          styleKey: d.render_style ?? null,
          faded: isInvalidated,
        };
        if (dispatchRenderGeometry(d.render_geometry, ctx)) {
          // Registry markers fold into the shared candle-series list so
          // the existing cleanup/sort pass further below handles them
          // uniformly with wyckoff/event markers.
          for (const m of regMarkers) allMarkers.push(m);
          continue;
        }
      }

      // Wyckoff has its own box + event overlay below — skip the generic
      // anchor-connecting polyline (purple zigzag) that adds noise.
      // Candle / gap render as highlight bands (block further down), not
      // polylines — a line through [open, close] is visually useless for
      // 2-bar patterns.
      const isBandFamily = d.family === "candle" || d.family === "gap";
      if (d.anchors.length >= 2 && !isZone && d.family !== "wyckoff" && !isBandFamily) {
        // P5 — for two-trendline classical patterns (rectangle, channel,
        // wedge, triangle, flag, pennant, diamond), split alternating
        // anchors into UPPER and LOWER polylines so the chart shows
        // proper trendlines instead of a zigzag through all pivots.
        const useTwoLines =
          d.family === "classical" &&
          d.anchors.length >= 4 &&
          isTwoTrendlinePattern(d.subkind);

        if (useTwoLines) {
          // Partition anchors into highs / lows based on price relative
          // to local neighbour. For 4 alternating pivots, the higher of
          // each adjacent pair belongs to the upper band.
          const upper: { time: Time; value: number }[] = [];
          const lower: { time: Time; value: number }[] = [];
          for (let i = 0; i < d.anchors.length; i++) {
            const a = d.anchors[i];
            const prev = d.anchors[i - 1];
            const next = d.anchors[i + 1];
            const ref =
              prev !== undefined ? Number(prev.price)
              : next !== undefined ? Number(next.price)
              : Number(a.price);
            const point = { time: isoToUnix(a.time), value: Number(a.price) };
            if (Number(a.price) >= ref) upper.push(point);
            else lower.push(point);
          }
          const mkLine = (pts: { time: Time; value: number }[]) => {
            if (pts.length < 2) return;
            const ln = chart.addSeries(LineSeries, {
              color: color,
              lineWidth: isInvalidated ? 1 : 2,
              lineStyle: isInvalidated ? LineStyle.Dotted : LineStyle.Solid,
              crosshairMarkerVisible: false,
              lastValueVisible: false,
              priceLineVisible: false,
              pointMarkersVisible: true,
              pointMarkersRadius: 3,
            });
            ln.setData(dedupeLineData(pts));
            overlayLinesRef.current.push(ln);
          };
          mkLine(upper);
          mkLine(lower);
        } else {
          // Default — single polyline through all anchors (double_top,
          // H&S, harmonic XABCD, elliott impulse, cup&handle, rounding).
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
        }

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

      // Zone box detections — render as RectanglePrimitive (same lib
      // Wyckoff box uses). Two-line rendering is deprecated: rectangles
      // give proper fill + border, auto-extend with chart, and match the
      // visual language of the Wyckoff trading-range box.
      if (isZone && d.anchors.length >= 2) {
        const p1 = Number(d.anchors[0].price);
        const p2 = Number(d.anchors[1].price);
        const top = Math.max(p1, p2);
        const bot = Math.min(p1, p2);
        // P19b — mitigated-zone hide (SMC-correct): a wick touch is
        // NOT mitigation; the zone is only "consumed" when a candle
        // CLOSES inside it (or beyond). This matches ICT/Wyckoff
        // literature: FVGs fill on close, OBs are considered active
        // until the body breaks through. Wick-only tests keep the
        // zone alive (they're the "test" that confirms the level).
        const formTime = isoToUnix(d.anchors[0].time) as number;
        let mitigated = false;
        for (let i = merged.candles.length - 1; i >= 0; i--) {
          const c = merged.candles[i];
          if (!c?.open_time) continue;
          const t = isoToUnix(c.open_time) as number;
          if (t <= formTime) break;
          const cl = Number(c.close);
          if (cl >= bot && cl <= top) {
            mitigated = true;
            break;
          }
        }
        if (mitigated) continue;
        const lastCandle = merged.candles[merged.candles.length - 1];
        if (!lastCandle?.open_time) continue;
        const startTime = isoToUnix(d.anchors[0].time);
        const endTime = isoToUnix(lastCandle.open_time);
        // Derive fill from subkind color with low alpha, border with
        // higher alpha. Hex color from familyColor() is already set
        // per-subkind (FVG=green/red, OB=blue/orange, LP/EQ=yellow/purple).
        const prim = new RectanglePrimitive({
          time1: startTime,
          time2: endTime,
          priceTop: top,
          priceBottom: bot,
          fillColor: `${color}1a`,      // ~10% alpha
          borderColor: `${color}80`,    // ~50% alpha
          borderWidth: 1,
          label: (d.subkind ?? "zone").replace(/_/g, " ").toUpperCase(),
          labelColor: color,
          labelSize: 9,
        });
        candleSeries.attachPrimitive(prim);
        rectanglePrimitivesRef.current.push(prim);
      }

      // Candle / gap highlight band — a filled rectangle covering the
      // pattern's bar span with the family/subkind color. Candles span
      // [open_first, close_last]; gaps span [pre_gap, post_gap]. Both
      // extend vertically to include invalidation_price (pattern extreme).
      if (isBandFamily && d.anchors.length >= 2) {
        const startTime = isoToUnix(d.anchors[0].time);
        const endTime = isoToUnix(d.anchors[d.anchors.length - 1].time);
        const prices = d.anchors.map((a) => Number(a.price));
        const inv = Number(d.invalidation_price);
        if (Number.isFinite(inv) && inv > 0) prices.push(inv);
        let top = Math.max(...prices);
        let bot = Math.min(...prices);
        if (top === bot) {
          // Degenerate band — spread by 0.1% so the rect is visible.
          const pad = top * 0.001;
          top += pad;
          bot -= pad;
        }
        const label = d.family === "gap"
          ? `GAP ${(d.subkind ?? "").replace(/_/g, " ").toUpperCase()}`
          : (d.subkind ?? d.family).replace(/_/g, " ").toUpperCase();
        const prim = new RectanglePrimitive({
          time1: startTime,
          time2: endTime,
          priceTop: top,
          priceBottom: bot,
          fillColor: `${color}26`,      // ~15% alpha
          borderColor: `${color}99`,    // ~60% alpha
          borderWidth: 1,
          label,
          labelColor: color,
          labelSize: 9,
        });
        candleSeries.attachPrimitive(prim);
        rectanglePrimitivesRef.current.push(prim);
      }

      // Entry / TP / SL price lines (only when layer enabled) —
      // formation-driven geometry with formation-specific labels
      // ("MM 1.0x", "Pat 1.618x", "ABCD 1.272x") so the chart shows
      // exactly the ladder the setup engine would arm. Generic
      // "TP1/TP2" labels are used only as a last-resort fallback.
      if (showDetail && layers.has("entry_tp_sl") && d.anchors.length > 0) {
        const { entry, sl, targets } = computeFormationTargets(d);
        const conf = Number(d.confidence) || 0;
        const confStr = conf > 0 ? ` (${(conf * 100).toFixed(0)}%)` : "";
        const lastTime = isoToUnix(d.anchors[d.anchors.length - 1].time);
        const barInterval = merged.candles.length >= 2
          ? (new Date(merged.candles[merged.candles.length - 1].open_time).getTime() -
             new Date(merged.candles[merged.candles.length - 2].open_time).getTime()) / 1000
          : 3600;
        const futureTime = (lastTime as number + barInterval * 20) as Time;
        // midpoint time for label positioning
        void 0; // midTime reserved for future label positioning

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
            // Exclude from Y-axis autoscale: otherwise TPs/SL that sit far from
            // the current price squash the candles into the top of the pane.
            autoscaleInfoProvider: () => null,
          });
          lvl.setData(sortLineData([
            { time: lastTime, value: price },
            { time: futureTime, value: price },
          ]));
          overlayLinesRef.current.push(lvl);
        };

        drawLevel(sl, "#ef4444", LineStyle.Dashed, "SL");
        drawLevel(entry, "#d4d4d8", LineStyle.Dotted, `Entry${confStr}`);
        // Use standardised TP1/TP2/TP3 labels. The formation-specific
        // name (e.g. "MM 1.618x", "ABCD 1.272x") is kept as the
        // tooltip on hover via a parenthesised suffix so the geometry
        // provenance is still discoverable without cluttering the
        // price scale.
        const tpPalette = ["#34d399", "#22c55ecc", "#22c55e80"];
        const tpStyles = [LineStyle.Dashed, LineStyle.Dotted, LineStyle.Dotted];
        targets.slice(0, 3).forEach((t, i) => {
          const label = `TP${i + 1} (${t.label})`;
          drawLevel(t.price, tpPalette[i] ?? "#22c55e60", tpStyles[i] ?? LineStyle.Dotted, label);
        });
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
        // Wyckoff has its own dedicated event markers (PS/SC/Spring/LPS/…) drawn
        // further below — skip the generic "Pn" anchor labels to avoid clutter.
        if (d.family === "wyckoff") continue;
        // In detail mode, respect the "labels" layer toggle
        const dLayers = detailLayers[d.family] ?? new Set(["entry_tp_sl", "labels"]);
        const isDetailMode = (familyModes[d.family] ?? "on") === "detail";
        if (isDetailMode && !dLayers.has("labels")) continue;
        const color = familyColor(d.family, d.subkind, debounced.timeframe);
        // P21b — TBM detections have a single anchor + signal strength
        // label. Drawing both a per-anchor circle ("Weak") and a
        // summary square ("bottom_setup 42%") on the same bar doubled
        // the glyph count for no added information. Collapse into one
        // square whose text carries the signal strength.
        if (d.family === "tbm") {
          const a = d.anchors[0];
          if (a) {
            const conf = Number(d.confidence) || 0;
            const confPct = conf > 0 ? ` ${(conf * 100).toFixed(0)}%` : "";
            const sig = a.label ? ` · ${a.label}` : "";
            allMarkers.push({
              time: isoToUnix(a.time),
              position: "belowBar" as const,
              color,
              shape: "square" as const,
              text: `${d.subkind}${confPct}${sig}`,
            });
          }
        } else {
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
              text: `${d.has_children ? "＋ " : ""}${
                d.family === "classical"
                  ? classicalSubkindLabel(d.subkind)
                  : d.subkind
              }${confPct}`,
            });
          }
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

    // ── Wyckoff event markers (AlphaExtract-style with overlap prevention) ──
    const wyckOverlay = wyckoffQuery.data?.overlay ?? null;
    if (wyckOverlay && (familyModes["wyckoff"] ?? "on") !== "off" && merged.candles?.length) {
      const wEvts = wyckOverlay.events ?? [];
      // Colors matching AlphaExtract indicator
      const eventMeta: Record<string, { label: string; color: string; pos: "aboveBar" | "belowBar" }> = {
        // Distribution (bearish) — reds/oranges
        p_s:          { label: "PS",     color: "#4CAF50", pos: "belowBar" },  // Preliminary Support (green)
        s_c:          { label: "SC",     color: "#45B39D", pos: "belowBar" },  // Selling Climax (turquoise)
        b_c:          { label: "BC",     color: "#FF7F00", pos: "aboveBar" },  // Buying Climax (orange)
        a_r:          { label: "AR",     color: "#2ECC71", pos: "aboveBar" },  // Automatic Rally (bright green)
        s_t:          { label: "ST",     color: "#66CDAA", pos: "belowBar" },  // Secondary Test (aquamarine)
        st_b:         { label: "ST-D",   color: "#FFA07A", pos: "aboveBar" },  // ST in Distribution (salmon)
        spring:       { label: "SPRING", color: "#00FA9A", pos: "belowBar" },  // Spring (spring green)
        u_a:          { label: "UT",     color: "#FFA500", pos: "aboveBar" },  // Upthrust (orange)
        utad:         { label: "UTAD",   color: "#FFA500", pos: "aboveBar" },  // UTAD (orange)
        shakeout:     { label: "TSO",    color: "#00FA9A", pos: "belowBar" },  // Terminal Shakeout (spring green)
        s_o_s:        { label: "SOS",    color: "#27AE60", pos: "aboveBar" },  // Sign of Strength (dark green)
        s_o_w:        { label: "SOW",    color: "#FF0000", pos: "belowBar" },  // Sign of Weakness (red)
        l_p_s:        { label: "LPS",    color: "#229954", pos: "belowBar" },  // Last Point of Support (forest green)
        lpsy:         { label: "LPSY",   color: "#FF4141", pos: "aboveBar" },  // Last Point of Supply (dark red)
        j_a_c:        { label: "JAC",    color: "#32CD32", pos: "aboveBar" },  // Jump Across Creek (lime)
        break_of_ice: { label: "BoI",    color: "#FF0000", pos: "belowBar" },  // Break of Ice (red)
        buec:         { label: "BUEC",   color: "#FF6347", pos: "aboveBar" },  // Backup After UT (tomato)
        s_o_t:        { label: "SOT",    color: "#008000", pos: "aboveBar" },  // (dark green)
        markup:       { label: "MU",     color: "#27AE60", pos: "aboveBar" },
        markdown:     { label: "MD",     color: "#FF0000", pos: "belowBar" },
      };

      // Overlap prevention (like AlphaExtract's hasNearbyLabel)
      // P-chart-fix — widened spacing and per-type cap to prevent marker
      // pileups (e.g. 9× s_c markers stacked on consecutive bars).
      const minSpacing = 10; // minimum bars between labels
      const placed: Array<{ idx: number; price: number }> = [];

      const hasNearbyLabel = (idx: number, price: number): boolean => {
        const cp = Number(merged!.candles[idx]?.close ?? 1);
        for (const p of placed) {
          if (Math.abs(idx - p.idx) < minSpacing && Math.abs(price - p.price) / cp < 0.005) {
            return true;
          }
        }
        return false;
      };

      // ── Frontend event detection (AlphaExtract logic) ──
      // Supplements backend events with Spring, LPS, SOS, AR, SOW, LPSY
      const candles = merged.candles;
      const frontendEvts: typeof wEvts = [];
      if (candles.length >= 25) {
        const volLen = 20;
        const priceLb = 20;
        const trendStr = 3;
        const volThresh = 2.0;
        const volFilter = 1.5;

        // Precompute helpers
        const cl = candles.map((c) => Number(c.close));
        const hi = candles.map((c) => Number(c.high));
        const lo = candles.map((c) => Number(c.low));
        const op = candles.map((c) => Number(c.open));
        const vol = candles.map((c) => Number(c.volume ?? 0));

        const sma = (arr: number[], len: number, i: number) => {
          if (i < len - 1) return arr[i] || 1;
          let s = 0; for (let j = i - len + 1; j <= i; j++) s += arr[j]; return s / len;
        };
        const highest = (arr: number[], len: number, i: number) => {
          let mx = -Infinity; for (let j = Math.max(0, i - len + 1); j <= i; j++) if (arr[j] > mx) mx = arr[j]; return mx;
        };
        const lowest = (arr: number[], len: number, i: number) => {
          let mn = Infinity; for (let j = Math.max(0, i - len + 1); j <= i; j++) if (arr[j] < mn) mn = arr[j]; return mn;
        };
        const falling = (i: number, n: number) => { for (let j = 1; j <= n && i - j >= 0; j++) if (cl[i - j] <= cl[i - j + 1] === false) return false; return true; };
        const rising = (i: number, n: number) => { for (let j = 1; j <= n && i - j >= 0; j++) if (cl[i - j] >= cl[i - j + 1] === false) return false; return true; };

        for (let i = priceLb + 1; i < candles.length; i++) {
          const volMA = sma(vol, volLen, i);
          const highestH = highest(hi, priceLb, i - 1);
          const lowestL = lowest(lo, priceLb, i - 1);
          const hv = vol[i] > volMA * volFilter;
          const range = hi[i] - lo[i] || 0.01;

          // Spring: break below support then close back above (hammer-like)
          if (lo[i] < lowest(lo, 3, i - 1) && cl[i] > op[i] && cl[i] > lo[i] + range * 0.6 && hv) {
            frontendEvts.push({ event: "spring", bar_index: i, price: lo[i], score: 0.8 });
          }
          // LPS: higher low + bullish close + low volume
          if (lo[i] > lo[i - 1] && cl[i] > op[i] && vol[i] < volMA * volFilter && rising(i, trendStr)) {
            frontendEvts.push({ event: "l_p_s", bar_index: i, price: lo[i], score: 0.6 });
          }
          // SOS: breakout above resistance + volume
          if (cl[i] > op[i] && hi[i] > highestH && vol[i] > volMA * volThresh && rising(i, trendStr)) {
            frontendEvts.push({ event: "s_o_s", bar_index: i, price: hi[i], score: 0.75 });
          }
          // SOW: breakdown below support + volume
          if (cl[i] < op[i] && lo[i] < lowestL && vol[i] > volMA * volThresh && falling(i, trendStr)) {
            frontendEvts.push({ event: "s_o_w", bar_index: i, price: lo[i], score: 0.75 });
          }
          // AR (Automatic Rally): strong bounce after SC
          if (cl[i] > op[i] && hi[i] > highest(hi, trendStr, i - 1) && vol[i] < volMA * volFilter && rising(i, trendStr)) {
            frontendEvts.push({ event: "a_r", bar_index: i, price: hi[i], score: 0.6 });
          }
          // LPSY: lower high + bearish + low volume
          if (hi[i] > hi[i - 1] && cl[i] < op[i] && vol[i] < volMA * volFilter && falling(i, trendStr)) {
            frontendEvts.push({ event: "lpsy", bar_index: i, price: hi[i], score: 0.6 });
          }
        }
      }

      // P-chart-fix — cap frontend events to 3 per type to avoid flooding
      // the chart with low-confidence duplicates.
      const MAX_PER_TYPE = 3;
      const feCountMap: Record<string, number> = {};
      const cappedFrontend = frontendEvts.filter((e) => {
        feCountMap[e.event] = (feCountMap[e.event] ?? 0) + 1;
        return feCountMap[e.event] <= MAX_PER_TYPE;
      });

      // Merge backend + frontend events, sort by score (backend first)
      const allWyckEvts = [...wEvts, ...cappedFrontend];
      const sortedEvts = allWyckEvts.sort((a, b) => b.score - a.score);

      // Event horizontal lines (like AlphaExtract's box.new for each event)
      const eventLines: Array<{ idx: number; price: number; color: string; label: string }> = [];

      for (const ev of sortedEvts) {
        const idx = ev.bar_index;
        if (idx < 0 || idx >= merged.candles.length) continue;
        const meta = eventMeta[ev.event] ?? { label: ev.event.toUpperCase(), color: "#9ca3af", pos: "aboveBar" as const };

        // Pin price to the candle's wick tip (Pine: high for top events, low for bottom).
        // Prevents the horizontal "shelf" from floating away from the candle body.
        const candle = merged.candles[idx];
        const pinnedPrice = meta.pos === "aboveBar" ? Number(candle.high) : Number(candle.low);
        if (hasNearbyLabel(idx, pinnedPrice)) continue;
        placed.push({ idx, price: pinnedPrice });

        const candleTime = isoToUnix(candle.open_time);
        allMarkers.push({
          time: candleTime,
          position: meta.pos,
          color: meta.color,
          shape: meta.pos === "aboveBar" ? "arrowDown" : "arrowUp",
          text: meta.label,
        });
        eventLines.push({ idx, price: pinnedPrice, color: meta.color, label: meta.label });
      }

      // P19e — removed horizontal "shelf" lines that led into each
      // Wyckoff event candle. They cluttered the chart (orphaned green
      // rails near SPRING/SC labels) and duplicated what the arrow
      // marker already conveys.
      void eventLines;

    }

    // ── Supplementary multi-structure event feed ──
    // /v2/wyckoff/events covers active + recent structures so the chart
    // still shows labels when no structure is currently active for
    // (symbol,TF). Runs in its own guard, independent of the
    // active-overlay block above. Events with a validator violation get
    // a yellow circle halo so the operator spots mis-placed events.
    const auditEvents = wyckEventsQuery.data?.events ?? [];
    if (
      auditEvents.length > 0 &&
      (familyModes["wyckoff"] ?? "on") !== "off" &&
      merged.candles?.length
    ) {
      // Reuse the same eventMeta dispatch as the active-overlay path.
      const auditEventMeta: Record<string, { label: string; color: string; pos: "aboveBar" | "belowBar" }> = {
        p_s:          { label: "PS",     color: "#4CAF50", pos: "belowBar" },
        s_c:          { label: "SC",     color: "#45B39D", pos: "belowBar" },
        b_c:          { label: "BC",     color: "#FF7F00", pos: "aboveBar" },
        a_r:          { label: "AR",     color: "#2ECC71", pos: "aboveBar" },
        s_t:          { label: "ST",     color: "#66CDAA", pos: "belowBar" },
        st_b:         { label: "ST-D",   color: "#FFA07A", pos: "aboveBar" },
        spring:       { label: "SPRING", color: "#00FA9A", pos: "belowBar" },
        u_a:          { label: "UT",     color: "#FFA500", pos: "aboveBar" },
        utad:         { label: "UTAD",   color: "#FFA500", pos: "aboveBar" },
        shakeout:     { label: "TSO",    color: "#00FA9A", pos: "belowBar" },
        s_o_s:        { label: "SOS",    color: "#27AE60", pos: "aboveBar" },
        s_o_w:        { label: "SOW",    color: "#FF0000", pos: "belowBar" },
        l_p_s:        { label: "LPS",    color: "#229954", pos: "belowBar" },
        lpsy:         { label: "LPSY",   color: "#FF4141", pos: "aboveBar" },
        j_a_c:        { label: "JAC",    color: "#32CD32", pos: "aboveBar" },
        break_of_ice: { label: "BoI",    color: "#FF0000", pos: "belowBar" },
        buec:         { label: "BUEC",   color: "#FF6347", pos: "aboveBar" },
        s_o_t:        { label: "SOT",    color: "#008000", pos: "aboveBar" },
        markup:       { label: "MU",     color: "#27AE60", pos: "aboveBar" },
        markdown:     { label: "MD",     color: "#FF0000", pos: "belowBar" },
      };
      const codeToSerde: Record<string, string> = {
        PS: "p_s", SC: "s_c", BC: "b_c", AR: "a_r", ST: "s_t",
        UA: "u_a", "ST-B": "st_b",
        Spring: "spring", UTAD: "utad", Shakeout: "shakeout",
        SpringTest: "spring", UTADTest: "utad",
        SOS: "s_o_s", SOW: "s_o_w", LPS: "l_p_s", LPSY: "lpsy",
        JAC: "j_a_c", BreakOfIce: "break_of_ice", BUEC: "buec",
        SOT: "s_o_t", Markup: "markup", Markdown: "markdown",
      };

      // Local dedup so we don't double-stamp markers the active-overlay
      // path already placed (those markers live in `allMarkers` after
      // the previous block ran).
      const placedKeys = new Set<string>();
      for (const m of allMarkers) {
        placedKeys.add(`${String(m.time)}|${m.position}`);
      }

      for (const ev of auditEvents) {
        const idx = ev.bar_index;
        if (idx === null || idx < 0 || idx >= merged.candles.length) continue;
        const serdeKey = codeToSerde[ev.event_code] ?? "";
        const meta = auditEventMeta[serdeKey] ?? {
          label: ev.event_code, color: "#9ca3af", pos: "aboveBar" as const,
        };
        const candle = merged.candles[idx];
        const candleTime = isoToUnix(candle.open_time);

        const dupKey = `${String(candleTime)}|${meta.pos}`;
        if (!placedKeys.has(dupKey)) {
          placedKeys.add(dupKey);
          allMarkers.push({
            time: candleTime,
            position: meta.pos,
            color: meta.color,
            shape: meta.pos === "aboveBar" ? "arrowDown" : "arrowUp",
            text: meta.label,
          });
        }

        if (ev.violation) {
          allMarkers.push({
            time: candleTime,
            position: meta.pos === "aboveBar" ? "belowBar" : "aboveBar",
            color: "#facc15", // amber-400 — matches Wyckoff page badge
            shape: "circle",
            text: `⚠ ${
              ev.violation.kind === "direction_conflict" ? "DIR"
              : ev.violation.kind === "phase_regression" ? "REG"
              : "LEAK"
            }`,
          });
        }
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
      const { range: wRange, creek, ice } = wyckoffOverlay;
      const isAccum = wyckoffOverlay.schematic === "accumulation" || wyckoffOverlay.schematic === "reaccumulation";
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

      // ── Authoritative Wyckoff range box ──
      //
      // The trading range (the "box") is defined by Phase A: AR sets the
      // top, SC/ST sets the bottom. Once Phase A completes, the range is
      // FROZEN — Springs and UTADs pierce it intentionally and must stay
      // outside. So we draw ONE box from `wyckoffOverlay.range` (which the
      // backend publishes from `WyckoffStructureTracker.range_top/bottom`),
      // spanning from `started_at` → latest bar.
      //
      // Previous versions drew RSI-based independent consolidation boxes;
      // that approach violates the Wyckoff rule set (RSI 30–70 mis-flags
      // trends as sideways, wick-based bounds inflate the box with
      // Spring/UT spikes, no Phase-A anchor). Removed.
      // P19f — guard against pathological overlays that caused the
      // "ACCUMULATION floating in empty space" bug: a razor-thin box
      // anchored to the last bar, or a vertically-oversized range
      // polluted by a spike. If `started_at` lands within the final few
      // bars, the box is meaningless — skip it. Likewise drop boxes
      // whose height exceeds 25% of current price (not a trading range,
      // that's the whole move).
      const lastPrice = Number(merged.candles[merged.candles.length - 1].close);
      const rangeHeight = (wRange.top ?? 0) - (wRange.bottom ?? 0);
      const barsFromStart = merged.candles.findIndex(
        (c) => isoToUnix(c.open_time) >= structStartT,
      );
      const barsSpan = barsFromStart >= 0 ? merged.candles.length - barsFromStart : 0;
      const boxTooThin = barsSpan < 5;
      const boxTooTall = lastPrice > 0 && rangeHeight / lastPrice > 0.25;
      // P-chart-fix — "DISTRIBUTION floating below price" bug: if current
      // price has moved >15% beyond the range, the structure is stale /
      // completed and drawing the box is misleading.
      const boxDisconnected =
        lastPrice > 0 &&
        ((wRange.top ?? 0) > 0 && lastPrice > (wRange.top ?? 0) * 1.15) ||
        ((wRange.bottom ?? 0) > 0 && lastPrice < (wRange.bottom ?? 0) * 0.85);
      const boxValid = !boxTooThin && !boxTooTall && !boxDisconnected;

      if (wRange.top != null && wRange.bottom != null && boxValid) {
        const schematicColor: Record<string, { fill: string; border: string; label: string }> = {
          accumulation:    { fill: "#22c55e14", border: "#22c55e70", label: "ACCUMULATION" },
          reaccumulation:  { fill: "#22c55e10", border: "#22c55e50", label: "RE-ACCUMULATION" },
          distribution:    { fill: "#ef444414", border: "#ef444470", label: "DISTRIBUTION" },
          redistribution:  { fill: "#ef444410", border: "#ef444450", label: "RE-DISTRIBUTION" },
        };
        const style = schematicColor[wyckoffOverlay.schematic] ?? {
          fill: "#6b728012",
          border: "#6b728050",
          label: "TRADING RANGE",
        };
        addRect({
          time1: structStartT,
          time2: lastT,
          priceTop: wRange.top,
          priceBottom: wRange.bottom,
          fillColor: style.fill,
          borderColor: style.border,
          borderWidth: 1,
          label: style.label,
          labelColor: style.border,
          labelSize: 10,
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
            // Keep the right-axis label but exclude from autoscale so distant
            // TPs don't squash the candles.
            autoscaleInfoProvider: () => null,
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

      // P19e — removed duplicate frontend FVG/IFVG detection block.
      // Backend qtss-range detector (rendered via ZONES family) already
      // handles FVG + IFVG with tighter thresholds, price-band dedup,
      // and close-based mitigation. Having a second frontend detector
      // drew redundant boxes inside Wyckoff view.
    }

    // ── Setup overlay (armed/active setups for current symbol+tf) ──
    // Each setup contributes 4 horizontal lines (SL / entry / koruma /
    // target_ref). We reuse the candle-range time axis; lines extend
    // from the first visible candle to ~20 bars past the last candle.
    if (showSetups && setupsQuery.data?.entries?.length && merged.candles?.length) {
      const firstT = isoToUnix(merged.candles[0].open_time) as number;
      const lastT = isoToUnix(merged.candles[merged.candles.length - 1].open_time) as number;
      const barStep = merged.candles.length >= 2
        ? (new Date(merged.candles[merged.candles.length - 1].open_time).getTime() -
           new Date(merged.candles[merged.candles.length - 2].open_time).getTime()) / 1000
        : 3600;
      const futureT = (lastT + barStep * 20) as Time;
      const startT = firstT as Time;
      const active = setupsQuery.data.entries.filter(
        (s) =>
          s.symbol === debounced.symbol &&
          s.timeframe === debounced.timeframe &&
          !s.close_reason &&
          (s.state === "armed" || s.state === "active" || s.state === "open"),
      );
      const profileColor: Record<string, string> = {
        T: "#38bdf8",
        Q: "#f59e0b",
        D: "#a855f7",
      };
      const drawSetupLevel = (price: number | null, col: string, style: LineStyle, label: string) => {
        if (price === null || !Number.isFinite(price)) return;
        const lvl = chart.addSeries(LineSeries, {
          color: col,
          lineWidth: 1,
          lineStyle: style,
          crosshairMarkerVisible: false,
          lastValueVisible: true,
          priceLineVisible: false,
          title: label,
          autoscaleInfoProvider: () => null,
        });
        lvl.setData(sortLineData([
          { time: startT, value: price },
          { time: futureT, value: price },
        ]));
        overlayLinesRef.current.push(lvl);
      };
      for (const s of active) {
        const col = profileColor[s.profile] ?? "#60a5fa";
        const dirTag = s.direction?.toUpperCase() === "LONG" ? "L" : s.direction?.toUpperCase() === "SHORT" ? "S" : "?";
        const tag = `${s.profile}${dirTag}`;
        const aiTag = s.ai_score != null ? ` ai=${(s.ai_score * 100).toFixed(0)}` : "";
        const trailTag = s.trail_mode ? " ⇢" : "";
        drawSetupLevel(s.entry_sl, "#ef4444aa", LineStyle.Dashed, `[${tag}] SL`);
        drawSetupLevel(s.entry_price, col, LineStyle.Solid, `[${tag}] Entry${aiTag}${trailTag}`);
        // Koruma (ratcheted protection stop) — only draw if distinct from
        // entry_sl so we don't stack two labels at the same price.
        if (
          s.koruma != null &&
          s.entry_sl != null &&
          Math.abs(s.koruma - s.entry_sl) > 1e-6
        ) {
          drawSetupLevel(s.koruma, "#fb923caa", LineStyle.Dotted, `[${tag}] Koruma`);
        }
        drawSetupLevel(s.target_ref, "#22c55e", LineStyle.Dashed, `[${tag}] Target`);
      }
    }

    // ── Live/Dry open positions overlay ─────────────────────────────
    // Surfaces currently-open positions (dry + live). entry_avg + current_sl
    // + tp_ladder are drawn; unrealized PnL appears on the entry label.
    if (showPositions && positionsQuery.data?.length && merged.candles?.length) {
      const firstT = isoToUnix(merged.candles[0].open_time) as number;
      const lastT = isoToUnix(merged.candles[merged.candles.length - 1].open_time) as number;
      const barStep = merged.candles.length >= 2
        ? (new Date(merged.candles[merged.candles.length - 1].open_time).getTime() -
           new Date(merged.candles[merged.candles.length - 2].open_time).getTime()) / 1000
        : 3600;
      const futureT = (lastT + barStep * 20) as Time;
      const startT = firstT as Time;
      const symMatches = positionsQuery.data.filter((p) => p.symbol === debounced.symbol);
      const drawPosLevel = (price: number | null, col: string, style: LineStyle, label: string) => {
        if (price === null || !Number.isFinite(price)) return;
        const lvl = chart.addSeries(LineSeries, {
          color: col,
          lineWidth: 2,
          lineStyle: style,
          crosshairMarkerVisible: false,
          lastValueVisible: true,
          priceLineVisible: false,
          title: label,
          autoscaleInfoProvider: () => null,
        });
        lvl.setData(sortLineData([
          { time: startT, value: price },
          { time: futureT, value: price },
        ]));
        overlayLinesRef.current.push(lvl);
      };
      for (const p of symMatches) {
        const side = (p.side || "").toUpperCase();
        const isLong = side === "BUY" || side === "LONG";
        const dirCol = isLong ? "#22c55e" : "#ef4444";
        const modeTag = p.mode.toUpperCase().slice(0, 3);
        const pnl = p.unrealized_pnl_quote != null ? Number(p.unrealized_pnl_quote) : null;
        const pnlTag =
          pnl == null
            ? ""
            : ` uPnL=${pnl >= 0 ? "+" : ""}${pnl.toFixed(2)}`;
        const lev = p.leverage && p.leverage > 1 ? ` ${p.leverage}x` : "";
        const tag = `[${modeTag} ${isLong ? "L" : "S"}${lev}]`;
        const entryAvg = Number(p.entry_avg);
        drawPosLevel(entryAvg, dirCol, LineStyle.Solid, `${tag} Entry${pnlTag}`);
        if (p.current_sl) {
          drawPosLevel(Number(p.current_sl), "#ef4444", LineStyle.Dashed, `${tag} SL`);
        }
        if (p.liquidation_price) {
          drawPosLevel(
            Number(p.liquidation_price),
            "#b91c1c",
            LineStyle.Dotted,
            `${tag} LIQ`,
          );
        }
        // tp_ladder format varies (array or object with `levels`). We tolerate
        // either: {price, label?} items or bare numbers.
        const ladderRaw = p.tp_ladder as
          | Array<{ price?: number; label?: string } | number>
          | { levels?: Array<{ price?: number; label?: string } | number> }
          | null
          | undefined;
        const ladderArr: Array<{ price?: number; label?: string } | number> = Array.isArray(
          ladderRaw,
        )
          ? ladderRaw
          : Array.isArray((ladderRaw as { levels?: unknown[] } | null)?.levels)
            ? ((ladderRaw as { levels: Array<{ price?: number; label?: string } | number> }).levels)
            : [];
        ladderArr.slice(0, 3).forEach((item, i) => {
          const price = typeof item === "number" ? item : item?.price;
          if (price == null || !Number.isFinite(price)) return;
          const lbl =
            typeof item === "object" && item?.label
              ? item.label
              : `TP${i + 1}`;
          drawPosLevel(Number(price), "#22c55ecc", LineStyle.Dotted, `${tag} ${lbl}`);
        });
      }
    }

  }, [merged, showVolume, showZigzag, showLabels, showProjections, visibleDetections, familyModes, detailLayers, wyckoffQuery.data, wyckEventsQuery.data, projectionsQuery.data, showSetups, showPositions, setupsQuery.data, positionsQuery.data, debounced.symbol, debounced.timeframe]);

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
          {/* Exchange / Segment / Symbol — chained combos from engine_symbols */}
          {(() => {
            const exchanges = Array.from(new Set(venueOptions.map((v) => v.exchange)));
            const segments = Array.from(
              new Set(venueOptions.filter((v) => v.exchange === form.venue).map((v) => v.segment)),
            );
            const currentVenue = venueOptions.find(
              (v) => v.exchange === form.venue && v.segment === form.segment,
            );
            const symbols = currentVenue?.symbols ?? [];
            const selectCls =
              "rounded border border-zinc-700 bg-zinc-900 px-1.5 py-0.5 text-[11px] text-zinc-200 focus:outline-none focus:border-zinc-500";
            return (
              <div className="flex items-center gap-1">
                <select
                  className={`w-24 ${selectCls}`}
                  value={form.venue}
                  onChange={(e) => {
                    const exch = e.target.value;
                    // Pick a valid segment + symbol for the new exchange.
                    const firstSeg =
                      venueOptions.find((v) => v.exchange === exch)?.segment ?? form.segment;
                    const firstSym =
                      venueOptions.find((v) => v.exchange === exch && v.segment === firstSeg)
                        ?.symbols[0] ?? form.symbol;
                    setForm({ ...form, venue: exch, segment: firstSeg, symbol: firstSym });
                  }}
                  disabled={venuesQuery.isLoading || exchanges.length === 0}
                  title="Exchange"
                >
                  {exchanges.length === 0 ? <option value={form.venue}>{form.venue}</option> : null}
                  {exchanges.map((x) => (
                    <option key={x} value={x}>
                      {x}
                    </option>
                  ))}
                </select>
                <select
                  className={`w-20 ${selectCls}`}
                  value={form.segment}
                  onChange={(e) => {
                    const seg = e.target.value;
                    const firstSym =
                      venueOptions.find((v) => v.exchange === form.venue && v.segment === seg)
                        ?.symbols[0] ?? form.symbol;
                    setForm({ ...form, segment: seg, symbol: firstSym });
                  }}
                  disabled={segments.length === 0}
                  title="Market type (spot / futures)"
                >
                  {segments.length === 0 ? (
                    <option value={form.segment}>{form.segment}</option>
                  ) : null}
                  {segments.map((s) => (
                    <option key={s} value={s}>
                      {s}
                    </option>
                  ))}
                </select>
                <select
                  className={`w-28 font-mono font-semibold ${selectCls}`}
                  value={form.symbol}
                  onChange={(e) => setForm({ ...form, symbol: e.target.value })}
                  disabled={symbols.length === 0}
                  title="Symbol"
                >
                  {symbols.length === 0 ? <option value={form.symbol}>{form.symbol}</option> : null}
                  {symbols.map((s) => (
                    <option key={s} value={s}>
                      {s}
                    </option>
                  ))}
                </select>
              </div>
            );
          })()}

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
                    {FAMILY_DISPLAY[family] ?? family}
                  </button>
                );
              })}
          </div>

          {/* Separator */}
          <div className="h-5 w-px bg-zinc-700" />

          {/* Overlay toggles — Setup + Open Position (backlog items).
              Separate from family toggles because they render from
              different APIs (/v2/setups, /v2/live-positions) and their
              lines are pinned horizontally across the whole window,
              not anchored to a pivot like detections. */}
          <div className="flex items-center gap-1">
            {(
              [
                { key: "setup", label: "SETUP", color: "#38bdf8", on: showSetups, toggle: () => setShowSetups((v) => !v), tip: "Armed/active setup'ların entry/SL/target çizgileri" },
                { key: "pos", label: "POZİSYON", color: "#22c55e", on: showPositions, toggle: () => setShowPositions((v) => !v), tip: "Açık paper/live pozisyon entry/SL/TP/LIQ çizgileri" },
              ] as const
            ).map((t) => (
              <button
                key={t.key}
                type="button"
                onClick={t.toggle}
                title={t.tip}
                className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] uppercase tracking-wide transition"
                style={{
                  borderWidth: 1,
                  borderColor: t.on ? t.color : "#3f3f46",
                  background: t.on ? `${t.color}18` : "transparent",
                  color: t.on ? t.color : "#71717a",
                }}
              >
                <span
                  className="inline-block h-1.5 w-2.5 rounded-sm"
                  style={{ background: t.on ? t.color : "#3f3f46" }}
                />
                {t.label}
              </button>
            ))}
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
                    {DETAIL_SUB_BUTTONS.filter(
                      (btn) => !btn.families || btn.families.includes(family),
                    ).map((btn) => {
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

        <div className="relative flex-1 bg-zinc-950" style={{ minHeight: 500 }}>
          <div
            ref={chartContainerRef}
            className="absolute inset-0"
            role="img"
            aria-label={`${debounced.symbol} ${debounced.timeframe} fiyat grafiği, ${visibleDetections.length} overlay`}
            tabIndex={0}
          />
          {/* Aşama 5.C — visually-hidden overlay descriptor for screen
              readers. The canvas itself has no semantic content so we
              mirror active detections into a list the AT can walk. */}
          <ul className="sr-only" aria-label="Aktif pattern overlay listesi">
            {visibleDetections.map(d => (
              <li key={d.id}>
                {d.family} / {d.subkind} · {d.state} · confidence{" "}
                {Number(d.confidence).toFixed(2)} · anchor{" "}
                {new Date(d.anchor_time).toISOString()}
              </li>
            ))}
          </ul>
          {/* ── Market Phase Panel (AlphaExtract style) ── */}
          {wyckoffQuery.data?.overlay && (familyModes["wyckoff"] ?? "on") !== "off" && (() => {
            const wo = wyckoffQuery.data.overlay;
            if (!wo) return null;
            const isAcc = wo.schematic === "accumulation" || wo.schematic === "reaccumulation";
            // P-chart-fix — Phase A/B schematic is provisional (mirrors
            // Wyckoff.tsx logic). Only Phase C+ commits the family call.
            // Pre-C we intentionally omit the schematic direction from
            // the headline so the card doesn't contradict the "Phase A"
            // detail row below (bug: previously showed "RANGE ·
            // redistribution?" alongside "Phase A", implying a direction
            // the tracker had not yet committed to).
            const phaseUpper = (wo.phase ?? "").toUpperCase();
            const phaseLocked = ["C", "D", "E"].includes(phaseUpper);
            const phaseColor = phaseLocked
              ? (isAcc ? "#22c55e" : "#ef4444")
              : "#9ca3af";
            const phaseText = phaseLocked
              ? (isAcc ? "ACCUMULATION" : "DISTRIBUTION")
              : `RANGE · PHASE ${phaseUpper || "?"}`;
            const conf = wo.confidence ? (wo.confidence * 100).toFixed(0) : "?";
            const strength = Number(conf) > 70 ? "STRONG" : Number(conf) > 40 ? "MODERATE" : "WEAK";
            const rangeP = wo.range.top && wo.range.bottom
              ? ((wo.range.top - wo.range.bottom) / wo.range.bottom * 100).toFixed(1) + "%"
              : "—";
            return (
              <div className="pointer-events-none absolute left-2 top-2 z-10 rounded border border-zinc-700 bg-zinc-900/90 text-[10px] backdrop-blur-sm">
                <div className="flex items-center gap-2 border-b border-zinc-700 px-3 py-1">
                  <span className="text-[11px] font-bold text-zinc-300">MARKET PHASE</span>
                  <span className="font-bold" style={{ color: phaseColor }}>{phaseText}</span>
                </div>
                <div className="grid grid-cols-2 gap-x-4 gap-y-0.5 px-3 py-1 text-zinc-400">
                  <span>Phase</span><span className="text-right text-zinc-200">{wo.phase || "—"}</span>
                  <span>Strength</span><span className="text-right text-zinc-200">{strength}</span>
                  <span>Confidence</span><span className="text-right text-zinc-200">{conf}%</span>
                  <span>Range</span><span className="text-right" style={{ color: phaseColor }}>{rangeP}</span>
                </div>
              </div>
            );
          })()}
        </div>

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
