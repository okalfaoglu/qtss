/**
 * ElliottChart — pure drawing layer.
 *
 * All Elliott logic (zigzag pivots, motive 12345, ABC correction, fib
 * band, break box, label fusion) runs on the Rust backend:
 *   * `GET /v2/zigzag/...`   — trailing-window pivots + provisional leg.
 *   * `GET /v2/elliott/...`  — motive/ABC/fib/break_box computed on the
 *                              same pivots via `qtss_elliott::luxalgo_pine_port`.
 *
 * This component only fetches those two endpoints and renders them on
 * the chart. Nothing about the Elliott state machine lives in the
 * browser anymore.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  createChart,
  CandlestickSeries,
  LineSeries,
  type IChartApi,
  type ISeriesApi,
  type CandlestickData,
  type LineData,
  type Time,
  type UTCTimestamp,
  ColorType,
  CrosshairMode,
  LineStyle,
} from "lightweight-charts";

import { apiFetch } from "../lib/api";
import { TextLabelPrimitive } from "../lib/text-label-primitive";
import { RectanglePrimitive } from "../lib/rectangle-primitive";
import { PolygonPrimitive } from "../lib/polygon-primitive";

// Shape mirrors qtss_elliott::luxalgo_pine_port::PinePortOutput.
// Snake-case matches serde's default (the Rust structs use plain
// derive(Serialize) without rename_all).
interface BackendPivotPoint {
  direction: number;
  bar_index: number;
  price: number;
  label_override?: string;
  hide_label?: boolean;
}
interface BackendBreakBox {
  left_bar: number; right_bar: number; top: number; bottom: number;
}
interface BackendNextMarker { direction: number; bar_index: number; price: number; }
interface BackendAbcPattern {
  direction: number;
  anchors: BackendPivotPoint[];
  invalidated: boolean;
  /// "zigzag" | "flat_regular" | "flat_expanded" | "flat_running"
  /// (server-side default is "zigzag" when B retracement < 90%).
  subkind?: string;
}
interface BackendMotivePattern {
  direction: number;
  anchors: BackendPivotPoint[];
  live: boolean;
  next_hint: boolean;
  abc: BackendAbcPattern | null;
  break_box: BackendBreakBox | null;
  next_marker: BackendNextMarker | null;
}
interface BackendBreakMarker { direction: number; bar_index: number; price: number; }
interface BackendFibBand {
  x_anchor: number; x_far: number; pole_top: number; pole_bottom: number;
  y_500: number; y_618: number; y_764: number; y_854: number; broken: boolean;
}
interface BackendTrianglePattern {
  direction: number;
  /// "triangle_contracting" | "triangle_expanding" | "triangle_barrier"
  subkind: string;
  anchors: BackendPivotPoint[];
  invalidated: boolean;
}
interface BackendLevelOutput {
  length: number;
  color: string;
  pivots: BackendPivotPoint[];
  motives: BackendMotivePattern[];
  break_markers: BackendBreakMarker[];
  fib_band: BackendFibBand | null;
  triangles?: BackendTrianglePattern[];
}
interface ElliottResponse {
  venue: string;
  symbol: string;
  timeframe: string;
  candles: Array<{ time: string; bar_index: number }>;
  bar_count: number;
  levels: BackendLevelOutput[];
}

type BackendPivot = {
  bar_index: number;
  time: string;
  direction: number;
  price: number;
  volume: number;
  swing_tag: "HH" | "HL" | "LL" | "LH" | null;
};

type BackendLevel = {
  slot: number;
  length: number;
  color: string;
  pivots: BackendPivot[];
  provisional_pivot: BackendPivot | null;
};

type BackendCandle = {
  time: string;
  open: string | number;
  high: string | number;
  low: string | number;
  close: string | number;
  volume: string | number;
  bar_index: number;
};

type ZigzagResponse = {
  venue: string;
  symbol: string;
  timeframe: string;
  candles: BackendCandle[];
  levels: BackendLevel[];
};

// Harmonic (Gartley / Bat / Butterfly / Crab / Cypher / ...) —
// mirrors both /v2/harmonic (live) and /v2/harmonic-db (persisted),
// same JSON shape so a single fetcher handles the source toggle.
interface HarmonicAnchor {
  bar_index: number;
  time: string;
  price: number;
  label: string; // "X" | "A" | "B" | "C" | "D"
}
interface HarmonicPattern {
  slot: number;
  subkind: string; // e.g. "cypher_bull", "gartley_bear"
  direction: number; // +1 bullish, -1 bearish
  start_bar: number;
  end_bar: number;
  start_time: string;
  end_time: string;
  invalidated: boolean;
  anchors: HarmonicAnchor[];
  score?: number;
  ratios?: { ab?: number; bc?: number; cd?: number; ad?: number };
  extension?: boolean;
}
interface HarmonicResponse {
  venue: string;
  symbol: string;
  timeframe: string;
  candles: Array<{ time: string; bar_index: number }>;
  patterns: HarmonicPattern[];
}

// ── /v2/chart workspace response — used by the auxiliary overlay pass
// (Classical / Range / Gap). Matches the Rust `ChartWorkspace` shape in
// `crates/qtss-api/src/routes/v2_chart.rs`. Only the `detections` slice
// is consumed here; the primary chart candles/zigzag still come from
// `/v2/zigzag` so we do not duplicate that payload.
interface ChartWorkspaceAnchor {
  time?: string;
  price?: string | number;
  bar_index?: number;
  label?: string;
  label_override?: string;
}
interface ChartWorkspaceDetection {
  id: string;
  kind: string;              // e.g. "classical", "range", "gap"
  family: string;            // same as kind for our writers
  subkind: string;           // e.g. "double_bottom_bull", "fvg:bullish_fvg"
  state: string;             // "forming" | "confirmed" | "invalidated"
  label?: string;
  anchors: ChartWorkspaceAnchor[];
  confidence?: string | number;
  anchor_price?: string | number;
  anchor_time?: string;
  pivot_level?: string;
  mode?: string;             // "live" | "dry" | "backtest"
  /// FAZ 25.4.A — pass-through raw_meta for families that need
  /// extra render data (Wyckoff phase, corrective_kind, etc.).
  raw_meta?: Record<string, unknown>;
}
interface ChartWorkspaceResponse {
  venue: string;
  symbol: string;
  timeframe: string;
  candles: Array<{ time: string; bar_index: number }>;
  detections: ChartWorkspaceDetection[];
}

type VenueOpt = {
  exchange: string;
  segment: string;
  symbols: string[];
  intervals: string[];
  symbol_intervals: Record<string, string[]>;
};

const TIMEFRAMES = ["1m", "3m", "5m", "15m", "30m", "1h", "4h", "1d", "1w"];

interface LevelSlot {
  length: number;
  color: string;
  enabled: boolean;
}

const DEFAULT_SLOTS: LevelSlot[] = [
  { length: 3,  color: "#ef4444", enabled: true },
  { length: 5,  color: "#3b82f6", enabled: true },
  { length: 8,  color: "#e5e7eb", enabled: false },
  { length: 13, color: "#f59e0b", enabled: false },
  { length: 21, color: "#a78bfa", enabled: false },
];

function dedupeByTime(pts: LineData[]): LineData[] {
  if (pts.length === 0) return pts;
  const out: LineData[] = [pts[0]];
  for (let i = 1; i < pts.length; i++) {
    if ((pts[i].time as number) === (out[out.length - 1].time as number)) {
      out[out.length - 1] = pts[i];
    } else {
      out.push(pts[i]);
    }
  }
  return out;
}

/**
 * Optional per-host overrides for the default toggle states. Used by
 * IQChart to hide ZigZag / Harmonic master / Range / Gap on first
 * render so the Elliott early-wave markers are easier to spot.
 * `undefined` keeps the existing default (true) to preserve behaviour
 * on /v2/chart exactly as before.
 */
export interface LuxAlgoChartDefaults {
  showZigzag?: boolean;
  showHarmonic?: boolean;
  showRange?: boolean;
  showGap?: boolean;
  /** Override Z1..Z5 enabled flags. 5-element bool array. */
  slotsEnabled?: [boolean, boolean, boolean, boolean, boolean];
  /** When true, drop the page-level `-m-6` and `h-[calc(100vh-57px)]`
   * wrapper so the chart fits its parent container instead of the
   * viewport. Used by IQChart split-view. Default false (standalone). */
  embedded?: boolean;
  /** Default for the "Only latest motive" toggle. /v2/chart keeps it
   * on (true); IQChart turns it off so the user sees ALL completed
   * motives at once — important for spotting in-progress patterns. */
  onlyLatestMotive?: boolean;
  /** Optional list of price levels to draw as horizontal price lines
   * on top of the candle series. Used by IQChart to surface IQ-D /
   * IQ-T entry/SL/TP triplets. Each entry renders one line; the
   * caller controls colour and label. Lines are detached and
   * recreated whenever the array reference changes. */
  priceLineOverlays?: Array<{
    price: number;
    color: string;
    title: string;
    lineWidth?: number;
    lineStyle?: "solid" | "dashed" | "dotted";
  }>;
  /** Notify parent every time the chart's symbol / exchange / segment /
   * tf changes. Used by IQChart so the WaveBarsPanel below the chart
   * stays in sync with the user's TF / symbol selection. */
  onContextChange?: (next: {
    exchange: string;
    segment: string;
    symbol: string;
    tf: string;
  }) => void;
}

export function LuxAlgoChart({
  defaults,
}: { defaults?: LuxAlgoChartDefaults } = {}) {
  const [exchange, setExchange] = useState("binance");
  const [segment, setSegment] = useState("futures");
  const [symbol, setSymbol] = useState("BTCUSDT");
  const [tf, setTf] = useState("4h");
  // FAZ 25.1 — keep IQChart's WaveBarsPanel in sync. Fires on any
  // venue / symbol / TF change. No-op when the host doesn't pass a
  // callback (standalone /v2/chart).
  useEffect(() => {
    defaults?.onContextChange?.({ exchange, segment, symbol, tf });
  }, [exchange, segment, symbol, tf, defaults?.onContextChange]);
  const [slots, setSlots] = useState<LevelSlot[]>(() => {
    if (!defaults?.slotsEnabled) return DEFAULT_SLOTS;
    return DEFAULT_SLOTS.map((s, i) => ({
      ...s,
      enabled: defaults.slotsEnabled![i] ?? s.enabled,
    }));
  });
  // Pull the canonical Z1..Z5 ladder (length + color) from the backend
  // so the toolbar mirrors the same `system_config.zigzag.slot_N` rows
  // the engine writers consume. Fires once on chart mount; the user's
  // enabled toggles persist (we only patch length + color, never
  // enabled). Falls back silently to DEFAULT_SLOTS if the request
  // fails — avoids breaking the chart when an old API is in front.
  interface BackendSlotConfig {
    slot: number;
    length: number;
    color: string;
  }
  const slotConfigs = useQuery<{ slots: BackendSlotConfig[] }>({
    queryKey: ["zigzag-slots"],
    queryFn: () => apiFetch("/v2/zigzag/slots"),
    staleTime: 5 * 60_000,
    refetchOnWindowFocus: false,
  });
  useEffect(() => {
    const incoming = slotConfigs.data?.slots;
    if (!incoming) return;
    setSlots((prev) =>
      prev.map((s, i) => {
        const cfg = incoming.find((c) => c.slot === i);
        if (!cfg) return s;
        return { ...s, length: cfg.length, color: cfg.color };
      }),
    );
  }, [slotConfigs.data]);
  const [showFibBand, setShowFibBand] = useState(true);
  const [showHhLl, setShowHhLl] = useState(false);
  const [onlyLatestMotive, setOnlyLatestMotive] = useState(
    defaults?.onlyLatestMotive ?? true
  );
  const [showZigzag, setShowZigzag] = useState(defaults?.showZigzag ?? true);
  const [showElliott, setShowElliott] = useState(true);
  // FAZ 25 PR-25A — early-wave Elliott markers (nascent / forming /
  // extended impulse). Persisted under pattern_family='elliott_early';
  // separate fetch from /v2/elliott so toggling does not touch the
  // existing motive/abc/triangle render path.
  const [showElliottEarly, setShowElliottEarly] = useState(true);
  const [fibExtend, setFibExtend] = useState(false);
  // TF-dependent default bar count. Picked so the visible chart
  // window is meaningful for each cadence (long-cycle Elliott reads
  // need years of history on 1d/1w; short scalp TFs only need a few
  // days). User can still override via the Filters panel below.
  function defaultBarLimit(timeframe: string): number {
    switch (timeframe) {
      case "1m":
        return 5000;
      case "3m":
      case "5m":
        return 4000;
      case "15m":
      case "30m":
        return 3000;
      case "1h":
        return 2000;
      case "4h":
        return 1500;
      case "1d":
        return 5000;
      case "1w":
        return 2000;
      case "1M":
        return 500;
      default:
        return 1000;
    }
  }
  const [barLimit, setBarLimit] = useState(() => defaultBarLimit("4h"));
  // Re-apply a fresh TF default whenever the timeframe changes. We
  // intentionally always reset on TF flip — different TFs need very
  // different bar counts to be useful. (1d on 1000 bars = ~3y, on
  // 5000 = ~13y; 1m on 5000 = ~3.5d, on 1000 = ~17h.) If the user
  // has zoomed back further than the default, the lazy-load handler
  // below will still bump up to 10k as they pan/zoom.
  useEffect(() => {
    setBarLimit(defaultBarLimit(tf));
  }, [tf]);

  // Per-formation toggles. All default on — the user can uncheck any
  // pattern family to prove the backend really detected the remaining
  // ones (rather than drawing defaults). A single formation off means
  // its line(s) and label(s) are skipped on render; the API response
  // itself is unaffected.
  const [showImpulse, setShowImpulse] = useState(true);
  const [showZigzagAbc, setShowZigzagAbc] = useState(true);
  const [showFlatRegular, setShowFlatRegular] = useState(true);
  const [showFlatExpanded, setShowFlatExpanded] = useState(true);
  const [showFlatRunning, setShowFlatRunning] = useState(true);
  const [showTriContracting, setShowTriContracting] = useState(true);
  const [showTriExpanding, setShowTriExpanding] = useState(true);
  const [showTriBarrier, setShowTriBarrier] = useState(true);

  // Data source for Elliott detections. 'live' recomputes via
  // luxalgo_pine_port::run on each request (canonical path, matches
  // /v2/elliott). 'db' reads the persisted `detections` table via
  // /v2/elliott-db. Flipping between the two should produce identical
  // output when the writer tick has caught up — visual diff is the
  // smoke test the user asked for.
  const [detectionSource, setDetectionSource] = useState<"live" | "db">("live");

  // Harmonic overlay — XABCD patterns (Gartley, Bat, Butterfly, Crab,
  // Cypher, ...). Drawn as a filled two-triangle "bow-tie" (XAB + BCD)
  // with labels and a green PRZ rectangle at D, matching the canonical
  // textbook rendering (Scott Carney). Source follows `detectionSource`
  // so the live/db toggle covers both families at once.
  const [showHarmonic, setShowHarmonic] = useState(defaults?.showHarmonic ?? true);

  // Per-pattern filter for the Harmonic family. Each key matches the
  // Rust spec name from qtss_harmonic::PATTERNS; the suffix _bull/_bear
  // is stripped before lookup. Defaults: all on.
  const HARMONIC_KINDS: Array<{ key: string; label: string }> = [
    { key: "gartley",      label: "Gartley" },
    { key: "bat",          label: "Bat" },
    { key: "alt_bat",      label: "Alt Bat" },
    { key: "butterfly",    label: "Butterfly" },
    { key: "crab",         label: "Crab" },
    { key: "deep_crab",    label: "Deep Crab" },
    { key: "shark",        label: "Shark" },
    { key: "cypher",       label: "Cypher" },
    { key: "five_zero",    label: "5-0" },
    { key: "ab_cd",        label: "AB=CD" },
    { key: "alt_ab_cd",    label: "Alt AB=CD" },
    { key: "three_drives", label: "Three Drives" },
  ];
  const [harmonicFilters, setHarmonicFilters] = useState<Record<string, boolean>>(
    () => Object.fromEntries(HARMONIC_KINDS.map((k) => [k.key, true]))
  );
  // Pattern target projections (Scott Carney: T1=0.382×CD, T2=0.618×CD,
  // T3=1.0×CD from D toward C). Drawn as horizontal lines extending
  // from D to chart end so operators see the Fibonacci TP ladder.
  const [showHarmonicTargets, setShowHarmonicTargets] = useState(true);

  // Toolbar filter rows (Elliott + Harmonic per-pattern toggles) are
  // collapsed by default so the chart gets maximum vertical space. The
  // primary controls (venue/symbol/tf/Z-slots/source) stay pinned.
  const [showFilters, setShowFilters] = useState(false);

  // Map subkind → toggle for the ABC classifier branch below. Keeps the
  // draw loop a pure look-up (CLAUDE.md #1: no scattered if/else).
  const abcVisibleFor = (subkind: string | undefined): boolean => {
    const key = subkind ?? "zigzag";
    switch (key) {
      case "zigzag": return showZigzagAbc;
      case "flat_regular": return showFlatRegular;
      case "flat_expanded": return showFlatExpanded;
      case "flat_running": return showFlatRunning;
      default: return true;
    }
  };
  const triangleVisibleFor = (subkind: string): boolean => {
    switch (subkind) {
      case "triangle_contracting": return showTriContracting;
      case "triangle_expanding": return showTriExpanding;
      case "triangle_barrier": return showTriBarrier;
      default: return true;
    }
  };

  const venues = useQuery<VenueOpt[]>({
    queryKey: ["chart-venues"],
    queryFn: () => apiFetch("/v2/chart/venues"),
  });

  const lengthsParam = slots.map((s) => s.length).join(",");
  const data = useQuery<ZigzagResponse>({
    queryKey: ["zigzag", exchange, symbol, tf, segment, lengthsParam, barLimit],
    queryFn: () =>
      apiFetch(
        `/v2/zigzag/${exchange}/${symbol}/${tf}?segment=${segment}&limit=${barLimit}&lengths=${lengthsParam}`
      ),
    refetchInterval: 15_000,
  });

  // Motive / ABC / Fib band / Break box — fetched from the backend
  // endpoint that runs the same Rust state machine on the same zigzag
  // pivots the /v2/zigzag route above is showing. No in-browser Elliott
  // computation anymore; the chart is pure presentation.
  const enabledLengths = slots.filter((s) => s.enabled).map((s) => s.length).join(",");
  const enabledColors = slots.filter((s) => s.enabled).map((s) => s.color).join(",");
  // Parallel array: original slot index for each length we send to the
  // backend. The Pine port returns a PACKED `levels[]` (one entry per
  // enabled slot), so when slot 2 (Z3) is OFF the returned levels[2]
  // is actually Z4's data, not Z3's. Without this remap the chart
  // paints Z4 in the Z3 row and drops Z5 entirely. Bug repro from the
  // user: "Z1,Z2,Z4,Z5 seçiliyse Z5 e ait veriler gözüküyor."
  const enabledSlotIndices: number[] = slots
    .map((s, i) => (s.enabled ? i : -1))
    .filter((i) => i >= 0);
  const elliott = useQuery<ElliottResponse>({
    queryKey: [
      "elliott",
      detectionSource,
      exchange,
      symbol,
      tf,
      segment,
      enabledLengths,
      enabledColors,
      barLimit,
    ],
    queryFn: () => {
      // DB source: reads persisted `detections` rows — no `lengths`
      // param because slots are baked into each row (slot column).
      // Live source: recomputes via the Pine port each request.
      if (detectionSource === "db") {
        return apiFetch(
          `/v2/elliott-db/${exchange}/${symbol}/${tf}?segment=${segment}&limit=${barLimit}`
        );
      }
      return apiFetch(
        `/v2/elliott/${exchange}/${symbol}/${tf}?segment=${segment}&limit=${barLimit}` +
          `&lengths=${enabledLengths}&colors=${encodeURIComponent(enabledColors)}`
      );
    },
    enabled: enabledLengths.length > 0,
    refetchInterval: 15_000,
  });
  const pineOutput = elliott.data ?? null;

  // FAZ 25 PR-25A — early-wave Elliott markers. Reads
  // /v2/elliott-early which is fed by the engine writer's elliott_early
  // sibling module (nascent + forming + extended impulse detection on
  // the same pivot tape). Strictly additive: the existing motive/abc
  // render path above is not touched.
  type EarlyMarker = {
    slot: number;
    subkind: string;
    stage: "nascent" | "forming" | "extended" | string;
    direction: number;
    start_bar: number;
    end_bar: number;
    start_time: string;
    end_time: string;
    anchors: Array<{
      bar_index: number;
      price: number;
      time?: string;
      direction: number;
      // FAZ 25.1 — backend stamps `label_override` ending in "?" on
      // anchors that are PROJECTED (Fib-simulated, not real pivots).
      // Frontend uses this to render dotted segments + dim labels.
      label_override?: string;
    }>;
    score: number;
    w3_extension: number;
    invalidation_price: number;
  };
  type EarlyResponse = {
    venue: string;
    symbol: string;
    timeframe: string;
    markers: EarlyMarker[];
  };
  const elliottEarly = useQuery<EarlyResponse>({
    queryKey: ["elliott-early", exchange, symbol, tf, segment, barLimit],
    queryFn: () =>
      apiFetch(
        `/v2/elliott-early/${exchange}/${symbol}/${tf}?segment=${segment}&limit=${barLimit}`
      ),
    enabled: showElliottEarly,
    refetchInterval: 15_000,
  });
  const earlyMarkers = elliottEarly.data?.markers ?? [];

  // Harmonic patterns — same source toggle as Elliott. The live and db
  // endpoints return identical JSON shape so one fetcher handles both.
  const harmonic = useQuery<HarmonicResponse>({
    queryKey: [
      "harmonic",
      detectionSource,
      exchange,
      symbol,
      tf,
      segment,
      barLimit,
    ],
    queryFn: () => {
      const path = detectionSource === "db" ? "/v2/harmonic-db" : "/v2/harmonic";
      return apiFetch(
        `${path}/${exchange}/${symbol}/${tf}?segment=${segment}&limit=${barLimit}`
      );
    },
    enabled: showHarmonic,
    refetchInterval: 30_000,
  });
  const harmonicOutput = harmonic.data ?? null;

  // ── Auxiliary detector overlays (Classical / Range / Gap) ───────────
  //
  // These families are persisted by the `qtss-engine` writers alongside
  // Elliott/Harmonic but have no dedicated `/v2/<family>` endpoint. The
  // chart workspace endpoint `/v2/chart/{v}/{s}/{tf}` already returns
  // every `detections` row filtered by mode + level, so we reuse it for
  // the auxiliary families. Mode filter (live/dry/backtest) is honoured
  // here — future dry/backtest runs will populate the same table with a
  // different `mode` value and appear as soon as this query picks them
  // up. The filter defaults to live but exposes all three so an
  // operator can visually diff strategies side-by-side.
  const [modeFilter, setModeFilter] = useState<"live" | "dry" | "backtest">("live");
  const [showClassical, setShowClassical] = useState(false);
  const [showRange, setShowRange] = useState(defaults?.showRange ?? true);
  const [showGap, setShowGap] = useState(defaults?.showGap ?? true);
  // Candlestick patterns (43 in the library). Default off to keep the
  // chart legible — 1h has ~3 candle marks per day; 4h a handful per
  // week. Enable when evaluating short-term reversals.
  const [showCandles, setShowCandles] = useState(false);
  // Opening Range Breakouts (Asia / London / NY session opens). Draws
  // the session's first-hour high/low as a horizontal band plus a
  // marker at the breakout close.
  const [showOrb, setShowOrb] = useState(false);
  // Smart Money Concepts (BOS / CHoCH / MSS / LiquiditySweep / FVI).
  // Single-anchor events rendered as short horizontal markers + labels.
  const [showSmc, setShowSmc] = useState(false);
  // FAZ 25.4.A — Wyckoff event overlay. Surfaces the 12-event
  // detector output (SC / BC / Spring / UTAD / SOS / SOW / AR / ST /
  // LPS / PS / BU / Test) with phase pill (A-E) and bull/bear
  // coloring so the operator sees institutional accumulation /
  // distribution footprints alongside the Elliott structure.
  const [showWyckoff, setShowWyckoff] = useState(false);
  // FAZ 25.4.E — per-element Wyckoff filter. The master `showWyckoff`
  // toggle gates the whole family; these sub-toggles let the operator
  // pick which elements actually paint when Wyckoff is on. Each
  // grouping has its own switch:
  //   - 4 cycle phases (Markup / Distribution / Markdown / Accumulation)
  //   - schematic ranges (Wyckoff Phase A-E inner box)
  //   - 12 individual events (climaxes, springs, sos/sow, ar/st, ...)
  //   - source filter (event-only / elliott-only / confluent-only)
  // Defaults: cycles + ranges on, events off (events are noisy at low Z).
  const [wyckoffFilter, setWyckoffFilter] = useState({
    cycle_markup: true,
    cycle_distribution: true,
    cycle_markdown: true,
    cycle_accumulation: true,
    ranges: true,
    events_climax: false,   // SC, BC
    events_spring: true,    // Spring, UTAD (Phase-C high-conviction)
    events_sos_sow: false,  // SOS, SOW
    events_other: false,    // AR, ST, LPS, PS, BU, Test
    src_event: true,
    src_elliott: true,
    src_confluent: true,
  });
  const [wyckoffFilterOpen, setWyckoffFilterOpen] = useState(false);
  // FAZ 25.x — overlay toggle for the elliott_full writer (diagonals,
  // flats, extended impulses, truncated fifth, W-X-Y combinations).
  // Default off because it's noisy on intraday tapes; operator opts in.
  const [showElliottFull, setShowElliottFull] = useState(false);
  // ── Technical indicator overlays (Faz 11 Aşama 5). Price-pane
  //    overlays only on this release — oscillators (RSI / Williams%R /
  //    CMF / Aroon / TTM Squeeze) land in PR-11H with a dedicated
  //    sub-pane + add/remove config panel. Each flag here maps to one
  //    indicator name the `/v2/indicators` endpoint understands.
  const [showSuperTrend, setShowSuperTrend] = useState(false);
  const [showKeltner, setShowKeltner] = useState(false);
  const [showIchimoku, setShowIchimoku] = useState(false);
  const [showDonchian, setShowDonchian] = useState(false);
  const [showPsar, setShowPsar] = useState(false);
  // Oscillators render in a dedicated sub-pane (paneIndex=1) below the
  // price pane. Each flag corresponds to an indicator name the /v2/
  // indicators endpoint knows. PR-11H.
  const [showRsi, setShowRsi] = useState(false);
  const [showWilliamsR, setShowWilliamsR] = useState(false);
  const [showCmf, setShowCmf] = useState(false);
  const [showAroon, setShowAroon] = useState(false);
  const [showTtmSqueeze, setShowTtmSqueeze] = useState(false);
  // 2026-04-26 user request: MACD missing from oscillators ("en
  // önemli olanı eklememişiz"). MACD + Stochastic + OBV + ATR are
  // staples; backend already serves them via /v2/indicators.
  const [showMacd, setShowMacd] = useState(false);
  const [showStochastic, setShowStochastic] = useState(false);
  const [showObv, setShowObv] = useState(false);
  const [showAtr, setShowAtr] = useState(false);
  const [showBollinger, setShowBollinger] = useState(false);
  const indicatorsCsv = [
    showSuperTrend && "supertrend",
    showKeltner && "keltner",
    showIchimoku && "ichimoku",
    showDonchian && "donchian",
    showPsar && "psar",
    showBollinger && "bollinger",
    showRsi && "rsi",
    showMacd && "macd",
    showStochastic && "stochastic",
    showWilliamsR && "williams_r",
    showCmf && "cmf",
    showAroon && "aroon",
    showTtmSqueeze && "ttm_squeeze",
    showObv && "obv",
    showAtr && "atr",
  ]
    .filter((x): x is string => typeof x === "string")
    .join(",");
  // Honour the Z-slot toggles so the chart doesn't drown in 2000+
  // classical detections when every slot is off but the checkboxes
  // above left them armed. Passing only enabled levels to the backend
  // also cuts the row scan on the SQL side.
  const levelsParamAux = slots
    .map((s, i) => (s.enabled ? `L${i}` : null))
    .filter((x): x is string => x !== null)
    .join(",");
  const auxWorkspace = useQuery<ChartWorkspaceResponse>({
    queryKey: [
      "chart-workspace",
      exchange,
      symbol,
      tf,
      segment,
      barLimit,
      modeFilter,
      levelsParamAux,
    ],
    queryFn: () =>
      apiFetch(
        `/v2/chart/${exchange}/${symbol}/${tf}?segment=${segment}&limit=${barLimit}` +
          `&modes=${modeFilter}&levels=${levelsParamAux}`,
      ),
    // No point firing when nothing is enabled — and with no levels in
    // the request the backend returns an empty slice anyway (the SQL
    // `slot = ANY($5)` clause would be vacuously false).
    enabled:
      (showClassical || showRange || showGap || showCandles || showOrb || showSmc ||
       showElliottFull || showWyckoff) &&
      levelsParamAux.length > 0,
    refetchInterval: 30_000,
  });
  // Cap the volume of auxiliary overlays we actually draw. Classical in
  // particular is noisy (sliding scan → thousands of near-duplicates);
  // showing the most recent N is the pragmatic default and keeps the
  // chart responsive. Operator can bump this via config later.
  //
  // FAZ 25.4.E — ALL Wyckoff rows bypass the cap. The 12 events (each
  // deduped per-subkind in proximity windows) plus cycles + ranges
  // are a small set (~50-100 rows total per symbol/tf). The cap was
  // designed for noisy classical / SMC scans that fire near-duplicates
  // every tick. User report: \"eventler seçili olduğu halde grafikte
  // gözükmüyor\" — events were getting cut by the AUX_RENDER_CAP=120
  // when SMC / classical / gap / candle saturated the top 120.
  //
  // Sub-cap on cycle rows per (slot, phase): keep only the LATEST 2.
  // Z3 chart screenshots showed multiple historical Markdown +
  // Accumulation tiles stacked, creating visual conflicts. Showing
  // only the most-recent tiles per phase per slot keeps the active
  // rotation visible without 6-month history clutter.
  const AUX_RENDER_CAP = 120;
  const CYCLES_PER_SLOT_PHASE = 2;
  const _allAuxDetections = auxWorkspace.data?.detections ?? [];
  const _wyckoffNonCycle = _allAuxDetections.filter(
    (d) =>
      d.family === "wyckoff" && !d.subkind.startsWith("cycle_"),
  );
  // Cycle rows: keep most-recent N per (slot, subkind) bucket.
  const _wyckoffCycles = _allAuxDetections.filter(
    (d) => d.family === "wyckoff" && d.subkind.startsWith("cycle_"),
  );
  const _cycleBucketCounts = new Map<string, number>();
  const _wyckoffCyclesKept = _wyckoffCycles
    .slice() // already start_time DESC from API
    .filter((d) => {
      const key = `${d.pivot_level ?? ""}::${d.subkind}`;
      const cnt = _cycleBucketCounts.get(key) ?? 0;
      if (cnt >= CYCLES_PER_SLOT_PHASE) return false;
      _cycleBucketCounts.set(key, cnt + 1);
      return true;
    });
  const _otherRows = _allAuxDetections
    .filter((d) => d.family !== "wyckoff")
    .slice(0, AUX_RENDER_CAP);
  const auxDetections = [
    ..._wyckoffNonCycle,
    ..._wyckoffCyclesKept,
    ..._otherRows,
  ];

  // ── /v2/indicators query — pulls the technical-indicator series map
  //    for whichever overlays the operator has toggled on. Aligned to
  //    the same bar series as /v2/zigzag via `bars[].bar_index` so the
  //    renderer can just lookup `data.data.candles[i].time` to place
  //    each point.
  interface IndicatorsResp {
    bars: Array<{ bar_index: number; time: string }>;
    series: Record<string, Record<string, number[]>>;
  }
  const indicators = useQuery<IndicatorsResp>({
    queryKey: [
      "indicators",
      exchange,
      symbol,
      tf,
      segment,
      barLimit,
      indicatorsCsv,
    ],
    queryFn: () =>
      apiFetch(
        `/v2/indicators/${exchange}/${symbol}/${tf}?segment=${segment}&limit=${barLimit}` +
          `&names=${indicatorsCsv}`,
      ),
    enabled: indicatorsCsv.length > 0,
    refetchInterval: 30_000,
  });

  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const candleSeriesRef = useRef<ISeriesApi<"Candlestick"> | null>(null);
  const overlaySeriesRef = useRef<ISeriesApi<"Line">[]>([]);
  const labelPrimitivesRef = useRef<TextLabelPrimitive[]>([]);
  const rectPrimitivesRef = useRef<RectanglePrimitive[]>([]);
  // Horizontal price-line overlays (IQ setup entry / SL / TP). Stored
  // as opaque `IPriceLine` handles so we can detach them on each
  // re-render before the new ones are attached. lightweight-charts
  // doesn't expose the type publicly so we use unknown[].
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const priceLineHandlesRef = useRef<any[]>([]);
  const polygonPrimitivesRef = useRef<PolygonPrimitive[]>([]);

  // Mount chart
  useEffect(() => {
    if (!containerRef.current) return;
    const chart = createChart(containerRef.current, {
      layout: {
        background: { type: ColorType.Solid, color: "#09090b" },
        textColor: "#d4d4d8",
      },
      grid: {
        vertLines: { color: "#18181b" },
        horzLines: { color: "#18181b" },
      },
      crosshair: { mode: CrosshairMode.Normal },
      rightPriceScale: { borderColor: "#27272a" },
      timeScale: {
        borderColor: "#27272a",
        timeVisible: true,
        secondsVisible: false,
        rightOffset: 12,
        barSpacing: 8,
        minBarSpacing: 2,
      },
    });
    chartRef.current = chart;
    candleSeriesRef.current = chart.addSeries(CandlestickSeries, {
      upColor: "#34d399",
      downColor: "#f87171",
      borderUpColor: "#34d399",
      borderDownColor: "#f87171",
      wickUpColor: "#34d39999",
      wickDownColor: "#f8717199",
    });
    const ro = new ResizeObserver((entries) => {
      for (const e of entries) {
        const { width, height } = e.contentRect;
        try { chart.applyOptions({ width, height }); } catch { /* disposed */ }
      }
    });
    ro.observe(containerRef.current);
    return () => {
      ro.disconnect();
      chart.remove();
      chartRef.current = null;
      candleSeriesRef.current = null;
    };
  }, []);

  // Pan/zoom-left: bump limit when visible range nears left edge.
  // Cap raised to 10k bars (was 5k) so daily / weekly users can zoom
  // out further without running out of data. Increment scales with
  // how close to the edge we are so far zooms catch up faster than
  // a single step.
  const loadOlder = useCallback((step: number) => {
    setBarLimit((n) => Math.min(n + step, 10_000));
  }, []);
  useEffect(() => {
    const chart = chartRef.current;
    if (!chart) return;
    const handler = (range: { from: number; to: number } | null) => {
      if (!range) return;
      // Bigger step the closer we are to the edge — covers zoom-out
      // jumps that span multiple page-fetches' worth at once.
      if (range.from < -50) loadOlder(2000);
      else if (range.from < 0) loadOlder(1000);
      else if (range.from < 20) loadOlder(500);
    };
    chart.timeScale().subscribeVisibleLogicalRangeChange(handler);
    return () => {
      try { chart.timeScale().unsubscribeVisibleLogicalRangeChange(handler); } catch { /* disposed */ }
    };
  }, [loadOlder]);

  // Redraw
  useEffect(() => {
    const chart = chartRef.current;
    const candleSeries = candleSeriesRef.current;
    if (!chart || !candleSeries || !data.data) return;

    for (const s of overlaySeriesRef.current) {
      try { chart.removeSeries(s); } catch { /* disposed */ }
    }
    overlaySeriesRef.current = [];
    for (const prim of labelPrimitivesRef.current) {
      try { candleSeries.detachPrimitive(prim); } catch { /* disposed */ }
    }
    labelPrimitivesRef.current = [];
    for (const prim of rectPrimitivesRef.current) {
      try { candleSeries.detachPrimitive(prim); } catch { /* disposed */ }
    }
    rectPrimitivesRef.current = [];
    for (const prim of polygonPrimitivesRef.current) {
      try { candleSeries.detachPrimitive(prim); } catch { /* disposed */ }
    }
    polygonPrimitivesRef.current = [];
    // Detach old price-line overlays before redrawing.
    for (const ph of priceLineHandlesRef.current) {
      try { candleSeries.removePriceLine(ph); } catch { /* disposed */ }
    }
    priceLineHandlesRef.current = [];

    // ETH 1d (and others) sometimes hit
    //   "Assertion failed: data must be asc ordered by time"
    // when the API returns rows with an out-of-order entry — usually
    // when the gap-fill loop re-publishes a stale row whose open_time
    // sits behind the rest of the page. Defensive sort + dedup by ts
    // keeps lightweight-charts happy regardless of upstream ordering.
    const candles = data.data.candles;
    const seenTs = new Set<number>();
    const candleData: CandlestickData[] = candles
      .map((c) => ({
        time: Math.floor(new Date(c.time).getTime() / 1000) as Time,
        open: Number(c.open),
        high: Number(c.high),
        low: Number(c.low),
        close: Number(c.close),
      }))
      .filter((row) => {
        const ts = row.time as unknown as number;
        if (seenTs.has(ts)) return false;
        seenTs.add(ts);
        return true;
      })
      .sort((a, b) => (a.time as unknown as number) - (b.time as unknown as number));
    candleSeries.setData(candleData);

    const timeAt = (barIndex: number): Time | null => {
      if (barIndex >= 0 && barIndex < candles.length) {
        return Math.floor(new Date(candles[barIndex].time).getTime() / 1000) as Time;
      }
      if (barIndex >= candles.length && candles.length >= 2) {
        const last = candles[candles.length - 1];
        const prev = candles[candles.length - 2];
        const lastTs = Math.floor(new Date(last.time).getTime() / 1000);
        const prevTs = Math.floor(new Date(prev.time).getTime() / 1000);
        const dt = lastTs - prevTs;
        return (lastTs + dt * (barIndex - (candles.length - 1))) as Time;
      }
      return null;
    };

    const attachLabel = (
      time: Time, price: number, text: string, color: string,
      position: "above" | "below"
    ) => {
      const prim = new TextLabelPrimitive({
        time, price, text, color, position,
        fontSize: 10, offsetPx: 8, paddingPx: 2,
        background: "#09090bcc",
      });
      candleSeries.attachPrimitive(prim);
      labelPrimitivesRef.current.push(prim);
    };

    // ── ZIGZAG (backend pivots) ──
    for (const level of data.data.levels) {
      const slot = slots[level.slot];
      if (!slot?.enabled) continue;
      const color = level.color;

      const zzPts: LineData[] = dedupeByTime(
        level.pivots
          .map((p) => {
            const t = timeAt(p.bar_index);
            return t === null ? null : { time: t, value: p.price };
          })
          .filter((x): x is LineData => x !== null)
      );
      if (showZigzag && zzPts.length >= 2) {
        const s = chart.addSeries(LineSeries, {
          color, lineWidth: 1, lineStyle: LineStyle.Dotted,
          priceLineVisible: false, lastValueVisible: false,
        });
        s.setData(zzPts);
        overlaySeriesRef.current.push(s);
      }

      // Provisional leg — dashed line from last confirmed pivot to the
      // current running extreme (TV's "pending" segment reaching the
      // right edge). Plus a Pine-style filled dot at the provisional
      // tip marking "this pivot is not yet confirmed".
      if (showZigzag && level.provisional_pivot && level.pivots.length > 0) {
        const lastConfirmed = level.pivots[level.pivots.length - 1];
        const prov = level.provisional_pivot;
        const tLast = timeAt(lastConfirmed.bar_index);
        const tProv = timeAt(prov.bar_index);
        if (tLast !== null && tProv !== null && (tProv as number) > (tLast as number)) {
          const s = chart.addSeries(LineSeries, {
            color, lineWidth: 2, lineStyle: LineStyle.Dashed,
            priceLineVisible: false, lastValueVisible: false,
          });
          s.setData([
            { time: tLast, value: lastConfirmed.price },
            { time: tProv, value: prov.price },
          ]);
          overlaySeriesRef.current.push(s);
          // Pine's red circle at the tip = "provisional / not yet
          // confirmed". We reuse the level color so a user with Z1..Z5
          // enabled can tell which level's tip is pending.
          attachLabel(
            tProv, prov.price, "●", color,
            prov.direction === 1 ? "above" : "below"
          );
        }
      }

      if (showHhLl) {
        for (const p of level.pivots) {
          if (!p.swing_tag) continue;
          const t = timeAt(p.bar_index);
          if (t === null) continue;
          attachLabel(t, p.price, p.swing_tag, color, p.direction === 1 ? "above" : "below");
        }
      }
    }

    // ── MOTIVE / ABC / FIB BAND / BREAK BOX (TS port) ──
    if (!pineOutput) return;
    for (let levelIdx = 0; levelIdx < pineOutput.levels.length; levelIdx++) {
      const level = pineOutput.levels[levelIdx];
      // Translate the API response index into the toolbar slot index.
      //
      // - `live` source: the API packs results — only enabled slots make
      //   it into the response, in send-order. levels[0] is the FIRST
      //   enabled slot, levels[1] the SECOND, etc. So levels[i] belongs
      //   to slot `enabledSlotIndices[i]`. (Bug fix: with Z3 unchecked,
      //   levels[2] used to leak into the Z3 row even though it carried
      //   Z4 data; meanwhile Z5's row stayed empty.)
      //
      // - `db` source: the response always has all 5 slots in canonical
      //   order; index maps 1:1 to slot index. We still gate by the
      //   toolbar enable flag so a single-Z view paints exactly one Z.
      const slotIdx =
        detectionSource === "live"
          ? enabledSlotIndices[levelIdx] ?? levelIdx
          : levelIdx;
      const slotCfgGate = slots[slotIdx];
      if (slotCfgGate && !slotCfgGate.enabled) continue;
      const color = level.color;
      // Elliott formations (motive + ABC + break box + markers) — a
      // single toggle covers the whole family. Fib band has its own.
      const motivesToDraw = showElliott
        ? (onlyLatestMotive ? level.motives.slice(0, 1) : level.motives)
        : [];
      for (const mw of motivesToDraw) {
        // Impulse toggle gates the 1-2-3-4-5 body. The motive's ABC is
        // still evaluated below and respects its own per-subkind toggle.
        if (showImpulse) {
          // STRICT: every anchor must map to a candle in the current
          // window. If ANY anchor times out (= candle for that bar
          // index isn't loaded), drop the whole motive — otherwise
          // the chart paints a half-motive that floats above the
          // empty pre-window void (user reported this on ETH 1d Z4
          // where an old motive's W1/W2 sat in the dead zone before
          // the chart's leftmost candle).
          const ptsRaw: Array<LineData | null> = mw.anchors.map((a) => {
            const t = timeAt(a.bar_index);
            return t === null ? null : { time: t, value: a.price };
          });
          if (ptsRaw.some((p) => p === null)) continue;
          const pts = ptsRaw as LineData[];
          const ptsClean = dedupeByTime(pts);
          if (ptsClean.length >= 2) {
            const style = mw.live ? LineStyle.Solid : LineStyle.Dotted;
            const s = chart.addSeries(LineSeries, {
              color, lineWidth: 2, lineStyle: style,
              priceLineVisible: false, lastValueVisible: false,
            });
            s.setData(ptsClean);
            overlaySeriesRef.current.push(s);

            if (mw.live) {
              const labels = ["(1)", "(2)", "(3)", "(4)", "(5)"];
              for (let i = 1; i < mw.anchors.length; i++) {
                const a = mw.anchors[i];
                // FAZ 25.4.E — was honouring `hide_label` from Pine
                // port's chained-motive logic. Pine hides W1/W2 of a
                // new motive when previous ABC's b/c shares the same
                // bar (label_override "(b)(1)" / "(c)(2)" replaces
                // them). But if the previous ABC isn't drawn (filter
                // off, or pruned by abcVisibleFor), the user sees
                // motives missing (1)/(2) labels — Claude audit:
                // \"(1)→(4)→(5) skipping (2),(3)\". Always render the
                // default wave label so every motive shows its full
                // (1)..(5) sequence. Duplicate labels at chained
                // bars are visually mild compared to missing waves.
                const t = timeAt(a.bar_index);
                if (t === null) continue;
                const aboveBar = (mw.direction === 1 && i % 2 === 1) || (mw.direction === -1 && i % 2 === 0);
                // Prefer override when present AND non-empty, else
                // fall back to the canonical wave label. Treats
                // empty-string override (Pine's suppress hint) the
                // same as null so it never blanks out.
                const override = a.label_override;
                const text =
                  typeof override === "string" && override.length > 0
                    ? override
                    : labels[i - 1];
                attachLabel(t, a.price, text, color, aboveBar ? "above" : "below");
              }
            }
          }
        }

        if (mw.abc && abcVisibleFor(mw.abc.subkind)) {
          const abcPts: LineData[] = mw.abc.anchors
            .map((a) => {
              const t = timeAt(a.bar_index);
              return t === null ? null : { time: t, value: a.price };
            })
            .filter((x): x is LineData => x !== null);
          const abcClean = dedupeByTime(abcPts);
          if (abcClean.length >= 2) {
            const abcStyle = mw.abc.invalidated ? LineStyle.Dashed : LineStyle.Solid;
            const abcSeries = chart.addSeries(LineSeries, {
              color, lineWidth: 2, lineStyle: abcStyle,
              priceLineVisible: false, lastValueVisible: false,
            });
            abcSeries.setData(abcClean);
            overlaySeriesRef.current.push(abcSeries);
          }
          if (!mw.abc.invalidated) {
            const abcLabels = ["(a)", "(b)", "(c)"];
            for (let i = 1; i < mw.abc.anchors.length; i++) {
              const a = mw.abc.anchors[i];
              if (a.hide_label) continue;
              const t = timeAt(a.bar_index);
              if (t === null) continue;
              const aboveBar = (mw.abc.direction === 1 && i % 2 === 1) || (mw.abc.direction === -1 && i % 2 === 0);
              const text = a.label_override ?? abcLabels[i - 1];
              attachLabel(t, a.price, text, color, aboveBar ? "above" : "below");
            }
          }
        }

        if (mw.break_box) {
          const bx = mw.break_box;
          const t1 = timeAt(bx.left_bar);
          const t2 = timeAt(bx.right_bar);
          if (t1 !== null && t2 !== null) {
            const rect = new RectanglePrimitive({
              time1: t1, time2: t2,
              priceTop: bx.top, priceBottom: bx.bottom,
              fillColor: `${color}1a`, borderColor: `${color}a6`, borderWidth: 1,
            });
            candleSeries.attachPrimitive(rect);
            rectPrimitivesRef.current.push(rect);
          }
        }

        if (mw.next_marker) {
          const t = timeAt(mw.next_marker.bar_index);
          if (t !== null) {
            attachLabel(t, mw.next_marker.price, "•", color,
              mw.next_marker.direction === 1 ? "above" : "below");
          }
        }
      }

      // Triangles (A-B-C-D-E, 3-3-3-3-3). Drawn as a single polyline
      // through all 6 anchors — the two converging (or diverging)
      // trendlines fall out of the alternating structure. Labels A..E
      // mark the 5 intermediate pivots.
      if (showElliott && level.triangles) {
        for (const tri of level.triangles) {
          if (!triangleVisibleFor(tri.subkind)) continue;
          const triPts: LineData[] = tri.anchors
            .map((a) => {
              const t = timeAt(a.bar_index);
              return t === null ? null : { time: t, value: a.price };
            })
            .filter((x): x is LineData => x !== null);
          const triClean = dedupeByTime(triPts);
          if (triClean.length < 2) continue;
          const triStyle = tri.invalidated ? LineStyle.Dashed : LineStyle.Solid;
          const tSeries = chart.addSeries(LineSeries, {
            color, lineWidth: 2, lineStyle: triStyle,
            priceLineVisible: false, lastValueVisible: false,
          });
          tSeries.setData(triClean);
          overlaySeriesRef.current.push(tSeries);
          const triLabels = ["", "A", "B", "C", "D", "E"];
          for (let i = 1; i < tri.anchors.length; i++) {
            const a = tri.anchors[i];
            if (a.hide_label) continue;
            const t = timeAt(a.bar_index);
            if (t === null) continue;
            // Labels alternate above/below matching the pivot direction.
            attachLabel(t, a.price, triLabels[i], color,
              a.direction === 1 ? "above" : "below");
          }
        }
      }

      if (showElliott) {
        for (const bm of level.break_markers) {
          const t = timeAt(bm.bar_index);
          if (t === null) continue;
          attachLabel(t, bm.price, "✕", "#ef4444", bm.direction === 1 ? "above" : "below");
        }
      }

      if (showFibBand && level.fib_band) {
        const fb = level.fib_band;
        const broken = fb.broken;
        const fillColor = broken ? "#ef444420" : "#10b98120";
        const tStart = timeAt(fb.x_anchor);
        // Default: 5 bars past p5 so the band doesn't dominate the chart.
        // `fibExtend` opts into Pine's ~10-bar extension + chart edge.
        const shortEnd = fb.x_anchor + 5;
        const tEnd = timeAt(fibExtend ? fb.x_far : shortEnd);
        // Guard against tStart === tEnd — lightweight-charts asserts
        // strictly-ascending time on setData; equal timestamps crash
        // the page. User report ETH 1h: \"Assertion failed: data must
        // be asc ordered by time, index=1, time=X, prev time=X\".
        if (
          tStart !== null &&
          tEnd !== null &&
          (tEnd as number) > (tStart as number)
        ) {
          const interp = (k: number) =>
            fb.y_500 + (fb.y_854 - fb.y_500) * (k - 0.5) / (0.854 - 0.5);
          const y_236 = interp(0.236);
          const y_382 = interp(0.382);
          const tuples: Array<[number, number, string]> = [
            [y_236, 26, "0.236"],
            [y_382, 38, "0.382"],
            [fb.y_500, 50, "0.500"],
            [fb.y_618, 62, "0.618"],
            [fb.y_764, 76, "0.764"],
            [fb.y_854, 85, "0.854"],
          ];
          for (const [price, alpha, label] of tuples) {
            const segColor = `${color}${Math.round(alpha * 2.55).toString(16).padStart(2, "0")}`;
            const s = chart.addSeries(LineSeries, {
              color: segColor, lineWidth: 1,
              lineStyle: broken ? LineStyle.Dotted : LineStyle.Solid,
              priceLineVisible: false, lastValueVisible: false,
            });
            s.setData([
              { time: tStart, value: price },
              { time: tEnd, value: price },
            ]);
            overlaySeriesRef.current.push(s);
            const labelPrim = new TextLabelPrimitive({
              time: tEnd, price, text: label, color: segColor,
              position: "above", fontSize: 9, offsetPx: 2, paddingPx: 2,
              background: "#09090bcc",
            });
            candleSeries.attachPrimitive(labelPrim);
            labelPrimitivesRef.current.push(labelPrim);
          }
          const rect = new RectanglePrimitive({
            time1: tStart, time2: tEnd,
            priceTop: Math.max(fb.y_764, fb.y_854),
            priceBottom: Math.min(fb.y_764, fb.y_854),
            fillColor, borderColor: fillColor, borderWidth: 0,
          });
          candleSeries.attachPrimitive(rect);
          rectPrimitivesRef.current.push(rect);
        }
      }
    }

    // ── ELLIOTT EARLY-WAVE MARKERS (FAZ 25 PR-25A) ────────────────────
    //
    // Nascent / forming / extended impulse signals from the engine
    // writer's elliott_early sibling module. Rendered as letter labels
    // at the LAST anchor of each match so the eye picks them up without
    // crowding the existing motive/abc/triangle drawings.
    //
    //   N  = nascent  (4 pivots; W3 broke W1 — earliest tradable signal)
    //   F  = forming  (5 pivots; W1+W2+W3+W4 in, W5 forming)
    //   X  = extended (full motive with one wave clearly extended)
    //
    // Bull marks below the bar in green, bear marks above in red.
    //
    // 2026-04-26 — User: "formasyon tikini kaldırınca simülasyon
    // kalkmadı. simülasyon elliott ile ilişkili değil." The
    // "Elliott formations" master toggle now ALSO gates the
    // elliott_early overlay (N/F/X markers + abc projection
    // dotted lines). N/F/X early stays as a sub-toggle that only
    // takes effect when the Elliott master is on.
    if (showElliott && showElliottEarly && earlyMarkers.length > 0) {
      const stageLetter: Record<string, string> = {
        nascent: "N",
        forming: "F",
        extended: "X",
        // ABC stages handled separately (multi-anchor render below);
        // these single-letter fallbacks only fire if anchors come up
        // empty for some reason.
        abc_nascent: "abc",
        abc_forming: "abc",
        abc_projected: "abc?",
      };
      // Pre-compute motive bar ranges per slot so we can suppress
      // nascent / forming markers that have since been promoted to a
      // full motive — otherwise the chart shows a stale N inside an
      // already-labeled (1)..(5). Extended markers (X) are kept since
      // they tag a property of the SAME motive (W3 / W5 extension).
      const motiveRangesBySlot: Map<number, Array<{ start: number; end: number }>> =
        new Map();
      if (pineOutput) {
        pineOutput.levels.forEach((lvl, levelIdx) => {
          // Same packed-vs-canonical remap as the motive draw loop above:
          // live source ships only enabled slots; db source ships all 5.
          // Without this remap motiveRangesBySlot stored ranges under
          // wrong keys when a non-contiguous slot set was selected,
          // leaving suppression of stale N/F markers in disabled slots
          // unreachable.
          const slotIdx =
            detectionSource === "live"
              ? enabledSlotIndices[levelIdx] ?? levelIdx
              : levelIdx;
          const ranges = lvl.motives.map((m) => {
            const bars = m.anchors.map((a) => a.bar_index);
            return { start: Math.min(...bars), end: Math.max(...bars) };
          });
          motiveRangesBySlot.set(slotIdx, ranges);
        });
      }
      // ABC dedup is enforced server-side in
      // crates/qtss-engine/src/writers/elliott_early.rs:write_early —
      // when a higher-stage row writes (forming > nascent > projected),
      // the lesser-stage rows for the same parent motive are deleted
      // in the same transaction. That guarantees the bot allocator
      // (which reads the table directly) and the GUI both see ONE row
      // per logical structure, no client-side filtering needed.
      for (const em of earlyMarkers) {
        // Slot filter follows the existing Z1..Z5 toolbar — early
        // markers respect the same slot enable as motives.
        const slotCfg = slots[em.slot];
        if (slotCfg && !slotCfg.enabled) continue;
        const last = em.anchors[em.anchors.length - 1];
        if (!last) continue;
        // Suppression: if this nascent / forming sits inside a complete
        // motive's bar range on the same slot, skip it. The motive's
        // (1)..(5) labels already convey what the N / F was hinting at.
        // For impulse N/F we suppress when a complete motive already
        // covers the same range. For ABC nascent/forming we do NOT
        // suppress — those fire AFTER a motive ends and are exactly
        // the in-progress correction the user wanted to see.
        if (em.stage === "nascent" || em.stage === "forming") {
          const ranges = motiveRangesBySlot.get(em.slot) ?? [];
          const inside = ranges.some(
            (r) => last.bar_index >= r.start && last.bar_index <= r.end
          );
          if (inside) continue;
        }

        // Helper: anchor → chart Time. Writer-stored ISO time is the
        // canonical reference; fall back to bar_index lookup only for
        // legacy rows that lack the time field.
        const anchorTime = (a: { bar_index: number; time?: string }): Time | null => {
          if (a.time) {
            const epoch = Math.floor(new Date(a.time).getTime() / 1000);
            if (!isNaN(epoch) && epoch > 0) return epoch as UTCTimestamp;
          }
          return timeAt(a.bar_index);
        };

        // ── ABC nascent / forming: real per-pivot labels + line
        //    segments instead of a single "a?" / "ab" letter.
        //    anchors[0] = parent motive's W5 (line origin, no label —
        //    the motive already shows "(5)" there).
        //    anchors[1] = A (label "(a)", solid line W5→A).
        //    anchors[2] = B (label "(b)", dotted line A→B).
        //    anchors[3] = C (label "(c)", dotted line B→C — forming).
        //    Each tick re-renders, so labels and lines reposition as
        //    the underlying pivot tape evolves until ABC locks in.
        if (
          em.stage === "abc_nascent" ||
          em.stage === "abc_forming" ||
          em.stage === "abc_projected"
        ) {
          const segLabels = ["", "(a)", "(b)", "(c)"];
          // Single colour — taken from the Z1..Z5 slot palette so the
          // ABC inherits the same colour as its motive lines. Real
          // vs projected are distinguished ONLY by line style
          // (solid vs dotted), per user request.
          const color = slots[em.slot]?.color ?? "#a855f7";

          const isProjectedAnchor = (a: { label_override?: string }) =>
            !!a.label_override && a.label_override.endsWith("?");

          // Backlog #5 (2026-04-26 BTC 4h Z4 (c)? at 70K vs spot 77K):
          // a projected anchor's price can fall in a zone the candles
          // never visited, painting a diagonal dotted line into empty
          // space. The 30%-of-full-chart-band tolerance from the
          // first cut was too generous — when an OLDER part of the
          // tape (e.g. the W4 low at 70K from weeks ago) sits within
          // the clip window, the simulation can still target it long
          // after price has moved on.
          //
          // Tightened version: clip against the LAST 20 BARS' high/low
          // (the user's eye-level "current swing"), with tolerance
          // capped at 1× the recent range. A projected anchor more
          // than one current-swing-band away from the latest tape is
          // visually misleading. Real (non-projected) anchors stay
          // unclipped — they reflect actual pivots.
          const clipWindow = candles.slice(Math.max(0, candles.length - 20));
          let chartMin = Infinity;
          let chartMax = -Infinity;
          for (const c of clipWindow) {
            const lo = Number(c.low);
            const hi = Number(c.high);
            if (Number.isFinite(lo) && lo < chartMin) chartMin = lo;
            if (Number.isFinite(hi) && hi > chartMax) chartMax = hi;
          }
          const chartBand = chartMax - chartMin;
          // 50% buffer (was 100%) — second user report (2026-04-26
          // Z5 white circle) showed (c)? projections still leaking
          // into zones the candles never visited even with the
          // tighter 20-bar window. Half-band tolerance suppresses
          // them. Real (non-projected) anchors stay unaffected.
          const tolerance = chartBand * 0.5;
          const priceWithinBand = (p: number, projected: boolean) => {
            if (!projected) return true;
            if (!Number.isFinite(chartMin) || !Number.isFinite(chartMax)) return true;
            return p >= chartMin - tolerance && p <= chartMax + tolerance;
          };

          const points: Array<{
            time: Time;
            price: number;
            idx: number;
            projected: boolean;
          }> = [];
          for (let i = 0; i < em.anchors.length; i++) {
            const a = em.anchors[i];
            const t = anchorTime(a);
            if (t === null) continue;
            const projected = isProjectedAnchor(a);
            if (!priceWithinBand(a.price, projected)) {
              // Out-of-band projected anchor — drop it AND every later
              // anchor, since the polyline downstream of an unreasonable
              // price would also be a fabrication.
              break;
            }
            points.push({
              time: t,
              price: a.price,
              idx: i,
              projected,
            });
          }
          if (points.length < 2) continue;

          // Each segment ENDING at a projected anchor is dotted; one
          // ending at a confirmed pivot is solid. Same colour for
          // both — the line style alone tells the user which legs
          // are committed.
          for (let i = 0; i < points.length - 1; i++) {
            const dest = points[i + 1];
            // Guard against equal timestamps (collapsed segment) —
            // lightweight-charts requires strictly-ascending time on
            // setData; equal timestamps trip the assertion and crash.
            if ((dest.time as number) <= (points[i].time as number)) continue;
            const series = chart.addSeries(LineSeries, {
              color,
              lineWidth: 2,
              lineStyle: dest.projected ? LineStyle.Dotted : LineStyle.Solid,
              priceLineVisible: false,
              lastValueVisible: false,
            });
            series.setData([
              { time: points[i].time, value: points[i].price },
              { time: dest.time, value: dest.price },
            ]);
            overlaySeriesRef.current.push(series);
          }

          for (const p of points) {
            if (p.idx === 0) continue;
            const baseText = segLabels[p.idx] ?? "?";
            const text = p.projected ? `${baseText}?` : baseText;
            attachLabel(
              p.time,
              p.price,
              text,
              color,
              em.direction === 1 ? "above" : "below"
            );
          }
          continue;
        }

        // ── Single-letter marker (N / F / X) at the last anchor.
        const t = anchorTime(last);
        if (t === null) continue;
        const letter = stageLetter[em.stage] ?? "?";
        const color = em.direction === 1 ? "#22c55e" : "#ef4444";
        attachLabel(t, last.price, letter, color, em.direction === 1 ? "below" : "above");
      }
    }

    // ── HARMONIC PATTERNS (XABCD) ─────────────────────────────────────
    //
    // Render style matches the Scott Carney reference: a two-triangle
    // "bow-tie" (△XAB + △BCD) filled with a semi-transparent sign-tinted
    // colour (blue for bullish, pink for bearish), the XABCD polyline
    // outline on top, X/A/B/C/D labels at each anchor, and a green PRZ
    // rectangle at D sized to the 0.786 XA retracement band — the
    // textbook reversal zone.
    //
    // One "bow-tie" per pattern row; patterns are fetched separately so
    // this block never blocks the Elliott path.
    if (showHarmonic && harmonicOutput) {
      for (const pat of harmonicOutput.patterns) {
        if (pat.anchors.length !== 5) continue;
        // Z-slot filter — only render patterns whose slot is enabled in
        // the Z1-Z5 toolbar (same switches that gate Elliott output).
        // slot 0 = Z1, slot 4 = Z5.
        const slotCfg = slots[pat.slot];
        if (slotCfg && !slotCfg.enabled) continue;
        // Per-pattern subkind filter (Gartley, Bat, ...). The DB stores
        // subkind with a `_bull` / `_bear` suffix — strip it and look
        // up the base name in the harmonicFilters map.
        const baseKind = pat.subkind.replace(/_(bull|bear)$/, "");
        if (harmonicFilters[baseKind] === false) continue;
        const pts: Array<LineData | null> = pat.anchors.map((a) => {
          const t = timeAt(a.bar_index);
          return t === null ? null : { time: t, value: a.price };
        });
        if (pts.some((p) => p === null)) continue;
        const clean = pts as LineData[];

        const bull = pat.direction === 1;
        const fill = bull ? "#3b82f640" : "#ec489940";   // 25% alpha
        const stroke = bull ? "#60a5faff" : "#f472b6ff"; // solid
        const prz = "#10b98133";                         // emerald 20% alpha
        const labelColor = bull ? "#60a5fa" : "#f472b6";

        // True polygon fill via PolygonPrimitive (pixel-space draw on
        // the candle pane). Lightweight-charts' primitive API handles
        // the per-frame time→pixel mapping so polygons stay locked to
        // their anchor bars through zoom/pan.
        const addTriangle = (i0: number, i1: number, i2: number) => {
          const t0 = timeAt(pat.anchors[i0].bar_index);
          const t1 = timeAt(pat.anchors[i1].bar_index);
          const t2 = timeAt(pat.anchors[i2].bar_index);
          if (t0 === null || t1 === null || t2 === null) return;
          const poly = new PolygonPrimitive({
            vertices: [
              { time: t0, price: pat.anchors[i0].price },
              { time: t1, price: pat.anchors[i1].price },
              { time: t2, price: pat.anchors[i2].price },
            ],
            fillColor: fill,
            borderColor: stroke,
            borderWidth: 1,
          });
          candleSeries.attachPrimitive(poly);
          polygonPrimitivesRef.current.push(poly);
        };

        // AB=CD family (classic + alternate) is a 4-point pattern — X
        // is a structural pivot but not part of the shape's rules.
        // Render △ABC + △BCD (triangles meeting along the BC edge),
        // solid A-B-C-D polyline, dotted enclosure lines A→C and B→D.
        // For XABCD patterns (Gartley, Bat, etc.) render the classic
        // Carney bow-tie: △XAB + △BCD.
        const isAbCdFamily =
          pat.subkind.startsWith("ab_cd") || pat.subkind.startsWith("alt_ab_cd");
        if (isAbCdFamily) {
          addTriangle(1, 2, 3); // △ABC
          addTriangle(2, 3, 4); // △BCD (= user's △CBD, same 3 points)
          // Dotted enclosure A→C and B→D. Each is its own short
          // LineSeries so the dash style is independent from the
          // main polyline.
          const dashSeries = (p0: LineData, p1: LineData) => {
            // Guard equal-time crash.
            if ((p1.time as number) <= (p0.time as number)) return;
            const s = chart.addSeries(LineSeries, {
              color: stroke,
              lineWidth: 1,
              lineStyle: LineStyle.Dotted,
              priceLineVisible: false,
              lastValueVisible: false,
            });
            s.setData([p0, p1]);
            overlaySeriesRef.current.push(s);
          };
          dashSeries(clean[1], clean[3]); // A → C
          dashSeries(clean[2], clean[4]); // B → D
        } else {
          addTriangle(0, 1, 2); // △XAB
          addTriangle(2, 3, 4); // △BCD
        }

        // Main polyline — solid A-B-C-D (AB=CD family, 4 points) or
        // X-A-B-C-D (XABCD family, 5 points). Dashed when invalidated.
        const polyPts = isAbCdFamily ? clean.slice(1) : clean;
        const poly = chart.addSeries(LineSeries, {
          color: stroke,
          lineWidth: 2,
          lineStyle: pat.invalidated ? LineStyle.Dashed : LineStyle.Solid,
          priceLineVisible: false,
          lastValueVisible: false,
        });
        poly.setData(polyPts);
        overlaySeriesRef.current.push(poly);

        // Labels — high pivots above, low pivots below. AB=CD skips the
        // "X" label since that anchor is structural only; XABCD shows all 5.
        // Pattern alternates: even indices (X, B, D) share X's kind,
        // odd (A, C) are the opposite. For a bullish XABCD the start X
        // is a low, so evens are lows; mirror for bearish.
        const labelStart = isAbCdFamily ? 1 : 0;
        for (let i = labelStart; i < pat.anchors.length; i++) {
          const a = pat.anchors[i];
          const t = timeAt(a.bar_index);
          if (t === null) continue;
          const evenShareIsLow = bull; // bull → X low → even=low
          const isLow = (i % 2 === 0) === evenShareIsLow;
          attachLabel(t, a.price, a.label, labelColor, isLow ? "below" : "above");
        }

        // PRZ — Potential Reversal Zone at D. Height ≈ 2% of the
        // pattern's primary leg (XA for XABCD, AB for AB=CD family)
        // — fib cluster tolerance Carney uses in "Harmonic Trading".
        // Anchored from D's bar to end of chart so the reader sees
        // whether price actually reverses off it.
        const xPrice = pat.anchors[0].price;
        const aPrice = pat.anchors[1].price;
        const bPrice = pat.anchors[2].price;
        const dPrice = pat.anchors[4].price;
        const refLeg = isAbCdFamily
          ? Math.abs(aPrice - bPrice) // AB leg
          : Math.abs(aPrice - xPrice); // XA leg
        const przHalf = refLeg * 0.02;
        const dBar = pat.anchors[4].bar_index;
        const dTime = timeAt(dBar);
        // PRZ extends forward ~(endBar - dBar) or 10 bars minimum.
        const lookahead = Math.max(10, Math.floor((data.data?.candles.length ?? 0) - dBar));
        const przEnd = timeAt(dBar + lookahead) ?? timeAt((data.data?.candles.length ?? 0) - 1);
        if (dTime !== null && przEnd !== null) {
          const przRect = new RectanglePrimitive({
            time1: dTime,
            time2: przEnd,
            priceTop: dPrice + przHalf,
            priceBottom: dPrice - przHalf,
            fillColor: prz,
            borderColor: prz,
            borderWidth: 0,
          });
          candleSeries.attachPrimitive(przRect);
          rectPrimitivesRef.current.push(przRect);
        }

        // ── Target projections (Scott Carney: T1 / T2 / T3) ──
        //
        // From D (pattern completion / entry), compute Fibonacci
        // retracements of the CD leg back toward C:
        //   T1 = D + 0.382 × (C − D)   minimum-move target
        //   T2 = D + 0.618 × (C − D)   moderate target
        //   T3 = D + 1.0   × (C − D)   full CD retrace (= C)
        // For a bullish pattern D is a low so targets are above D; for
        // a bearish pattern D is a high so targets are below. The
        // signed multiplier c-d already bakes in the direction.
        //
        // Target lines are short (6 bars forward from D) so they don't
        // dominate the chart — matches the Fib band's default short
        // extension. Deepens just enough for the operator to eyeball
        // where T1/T2/T3 sit relative to the subsequent candles.
        const TARGET_BARS_FORWARD = 6;
        const targetEnd =
          timeAt(dBar + TARGET_BARS_FORWARD)
          ?? timeAt((data.data?.candles.length ?? 0) - 1);
        if (
          showHarmonicTargets &&
          dTime !== null &&
          targetEnd !== null &&
          (targetEnd as number) > (dTime as number)
        ) {
          const cPrice = pat.anchors[3].price;
          const cdLeg = cPrice - dPrice; // signed (positive=bull)
          const targetColor = bull ? "#10b981" : "#ef4444"; // emerald / red
          const TARGETS: Array<[number, string]> = [
            [0.382, "T1 0.382"],
            [0.618, "T2 0.618"],
            [1.0,   "T3 1.0"],
          ];
          for (const [frac, label] of TARGETS) {
            const price = dPrice + cdLeg * frac;
            const line = chart.addSeries(LineSeries, {
              color: targetColor,
              lineWidth: 1,
              lineStyle: LineStyle.Dashed,
              priceLineVisible: false,
              lastValueVisible: false,
            });
            line.setData([
              { time: dTime, value: price },
              { time: targetEnd, value: price },
            ]);
            overlaySeriesRef.current.push(line);
            const lbl = new TextLabelPrimitive({
              time: targetEnd,
              price,
              text: `${label}  ${price.toFixed(2)}`,
              color: targetColor,
              position: "above",
              fontSize: 10,
              offsetPx: 2,
              paddingPx: 2,
              background: "#09090bcc",
            });
            candleSeries.attachPrimitive(lbl);
            labelPrimitivesRef.current.push(lbl);
          }
        }

        // Pattern name label — anchored at D (anchors[4]), the pattern's
        // completion / entry point. This is the bar the trader actually
        // cares about; placing the name there makes it trivial to scan
        // the chart for "where is the reversal now?". For a bullish
        // pattern D is a low so the label goes above (won't overlap the
        // PRZ rectangle anchored below); bearish mirrors.
        const nameAnchorIdx = 4;
        const tD = timeAt(pat.anchors[nameAnchorIdx].bar_index);
        if (tD !== null) {
          const niceName = pat.subkind
            .replace(/_/g, " ")
            .replace(/\b\w/g, (c) => c.toUpperCase());
          attachLabel(
            tD,
            pat.anchors[nameAnchorIdx].price,
            niceName,
            labelColor,
            bull ? "above" : "below",
          );
        }
      }
    }

    // ── AUXILIARY DETECTORS (Classical / Range / Gap) ─────────────────
    //
    // Sourced from the `/v2/chart` workspace endpoint — one network
    // round-trip returns every row that `qtss-engine`'s writers
    // persisted for the current venue/symbol/timeframe filtered by
    // mode. Render semantics per family:
    //
    //   classical  → polyline through anchors, name label at the last
    //                anchor (the pattern's completion bar)
    //   range      → horizontal band between first two anchors (zone
    //                high/low), name label at the left edge
    //   gap        → vertical marker at the gap bar, name label above
    //
    // Each family is gated by its own toggle and the shared mode filter.
    // Anchors arrive as `{time, price, label|label_override, bar_index}`.
    // Prices are strings (Rust Decimal) so we parseFloat defensively.
    const parsePrice = (v: unknown): number => {
      if (typeof v === "number") return v;
      if (typeof v === "string") return Number.parseFloat(v);
      return NaN;
    };
    const anchorTime = (a: ChartWorkspaceAnchor): Time | null => {
      // Prefer the absolute datetime over bar_index. Writers persist
      // both, but bar_index is in the WRITER's bar slice frame
      // (typically 2000 bars) which may not match the CHART's candles
      // array (e.g. 1500 bars at 4h) — passing a writer-frame
      // bar_index into timeAt() triggers the extrapolation path and
      // returns a future timestamp far off the right edge, so the
      // primitive renders off-screen and looks "missing". `time` is
      // an invariant ISO string that maps the same regardless of
      // frame.
      if (typeof a.time === "string") {
        const ts = Math.floor(new Date(a.time).getTime() / 1000);
        if (!Number.isNaN(ts)) return ts as UTCTimestamp;
      }
      if (typeof a.bar_index === "number") {
        const t = timeAt(a.bar_index);
        if (t !== null) return t;
      }
      return null;
    };
    const variantFromSubkind = (s: string): "bull" | "bear" | "neutral" => {
      const lower = s.toLowerCase();
      if (
        lower.includes("bull") ||
        lower.endsWith("_low") ||
        lower.endsWith("_bottom") ||
        lower.startsWith("bullish_") ||
        lower === "equal_lows"
      ) {
        return "bull";
      }
      if (
        lower.includes("bear") ||
        lower.endsWith("_high") ||
        lower.endsWith("_top") ||
        lower.startsWith("bearish_") ||
        lower === "equal_highs"
      ) {
        return "bear";
      }
      return "neutral";
    };
    const colorFor = (variant: "bull" | "bear" | "neutral") =>
      variant === "bull"
        ? "#22c55e"
        : variant === "bear"
          ? "#ef4444"
          : "#a1a1aa";
    const labelFor = (d: ChartWorkspaceDetection): string =>
      d.subkind
        .replace(/[:_]+/g, " ")
        .replace(/\b\w/g, (c) => c.toUpperCase())
        .trim();

    // Render each family through its own small adapter — no scattered
    // match on family strings at the call-site (CLAUDE.md #1).
    const renderClassical = (d: ChartWorkspaceDetection) => {
      if (d.anchors.length < 2) return;
      const pts: LineData[] = d.anchors
        .map((a) => {
          const t = anchorTime(a);
          const p = parsePrice(a.price);
          return t !== null && !Number.isNaN(p) ? { time: t, value: p } : null;
        })
        .filter((x): x is LineData => x !== null);
      if (pts.length < 2) return;
      const clean = dedupeByTime(pts);
      if (clean.length < 2) return;
      const variant = variantFromSubkind(d.subkind);
      const color = colorFor(variant);
      const style = d.state === "invalidated" ? LineStyle.Dashed : LineStyle.Solid;
      const s = chart.addSeries(LineSeries, {
        color,
        lineWidth: 2,
        lineStyle: style,
        priceLineVisible: false,
        lastValueVisible: false,
      });
      s.setData(clean);
      overlaySeriesRef.current.push(s);
      const tip = clean[clean.length - 1];
      attachLabel(
        tip.time,
        tip.value,
        labelFor(d),
        color,
        variant === "bull" ? "above" : "below",
      );
    };

    const renderRange = (d: ChartWorkspaceDetection) => {
      if (d.anchors.length < 2) return;
      const t0 = anchorTime(d.anchors[0]);
      if (t0 === null) return;
      const p0 = parsePrice(d.anchors[0].price);
      const p1 = parsePrice(d.anchors[1].price);
      if (Number.isNaN(p0) || Number.isNaN(p1)) return;
      const top = Math.max(p0, p1);
      const bot = Math.min(p0, p1);
      // Anchor the zone from the detection bar forward to the live edge
      // so the reader sees whether price has revisited it. 50-bar
      // lookahead fits most intraday timeframes; for longer TFs the
      // candle tape usually truncates before that anyway.
      const last = candles.length - 1;
      const lookBars =
        typeof d.anchors[0].bar_index === "number" ? d.anchors[0].bar_index + 50 : last;
      const tEnd = timeAt(Math.min(lookBars, last));
      if (tEnd === null) return;
      const variant = variantFromSubkind(d.subkind);
      const base = colorFor(variant);
      const fill = base + "22"; // 13% alpha
      const rect = new RectanglePrimitive({
        time1: t0,
        time2: tEnd,
        priceTop: top,
        priceBottom: bot,
        fillColor: fill,
        borderColor: base,
        borderWidth: 1,
      });
      candleSeries.attachPrimitive(rect);
      rectPrimitivesRef.current.push(rect);
      // Label at zone's upper-left (or lower-left for bearish zones).
      attachLabel(
        t0,
        variant === "bear" ? bot : top,
        labelFor(d),
        base,
        variant === "bear" ? "below" : "above",
      );
    };

    const renderGap = (d: ChartWorkspaceDetection) => {
      // A gap row has {P, G, [I]} anchors — render a short vertical
      // marker line from G's close up/down to signal direction, plus
      // the pattern name label. Island reversal's partner bar (I) gets
      // an extra marker so the two-gap structure is visible at a glance.
      if (d.anchors.length === 0) return;
      const gapAnchor =
        d.anchors.find((a) => (a.label || a.label_override) === "G") ?? d.anchors[0];
      const tG = anchorTime(gapAnchor);
      if (tG === null) return;
      const p = parsePrice(gapAnchor.price);
      if (Number.isNaN(p)) return;
      const variant = variantFromSubkind(d.subkind);
      const color = colorFor(variant);
      // 1.5% marker height — visible on any TF without dominating the
      // chart. Sign follows variant so bull markers point up.
      const sign = variant === "bull" ? 1 : variant === "bear" ? -1 : 0;
      const s = chart.addSeries(LineSeries, {
        color,
        lineWidth: 3,
        lineStyle: LineStyle.Solid,
        priceLineVisible: false,
        lastValueVisible: false,
      });
      // Render a 5-bar horizontal accent at the gap price instead of a
      // vertical at a single bar — lightweight-charts asserts strictly
      // ascending time so two points at tG would crash.
      const barIdx =
        typeof gapAnchor.bar_index === "number"
          ? gapAnchor.bar_index
          : candles.length - 1;
      const tEnd = timeAt(Math.min(barIdx + 5, candles.length - 1));
      if (tEnd !== null && (tEnd as number) > (tG as number)) {
        s.setData([
          { time: tG, value: p },
          { time: tEnd, value: p },
        ]);
        overlaySeriesRef.current.push(s);
      }
      // Use sign in label position so bull/bear gap glyph still
      // visually distinct without the vertical line trick.
      void sign;
      attachLabel(
        tG,
        p,
        labelFor(d),
        color,
        variant === "bear" ? "below" : "above",
      );
    };

    // Candlestick pattern — the library anchors on {open_of_first,
    // close_of_last}. Render a short glyph line from first-open to
    // last-close (2px line) with the pattern label above/below based on
    // variant. This is the "gesture" a trader sees on TradingView's
    // candle-pattern scanner: a small pointer without drowning the
    // price action.
    const renderCandle = (d: ChartWorkspaceDetection) => {
      if (d.anchors.length === 0) return;
      const first = d.anchors[0];
      const last = d.anchors[d.anchors.length - 1];
      const t1 = anchorTime(first);
      const t2 = anchorTime(last);
      const p1 = parsePrice(first.price);
      const p2 = parsePrice(last.price);
      if (t1 === null || t2 === null || Number.isNaN(p1) || Number.isNaN(p2)) return;
      const variant = variantFromSubkind(d.subkind);
      const color = colorFor(variant);
      // Single-bar patterns (doji, hammer, shooting_star …) have
      // identical start/end time, which lightweight-charts rejects for
      // a LineSeries. In that case just drop a labelled dot; multi-bar
      // patterns (morning-star, three-soldiers, engulfing) get a short
      // gesture line from first-open to last-close.
      if (t1 !== t2) {
        const s = chart.addSeries(LineSeries, {
          color,
          lineWidth: 2,
          lineStyle: LineStyle.Solid,
          priceLineVisible: false,
          lastValueVisible: false,
        });
        s.setData([
          { time: t1, value: p1 },
          { time: t2, value: p2 },
        ]);
        overlaySeriesRef.current.push(s);
      }
      attachLabel(
        t2,
        p2,
        labelFor(d),
        color,
        variant === "bear" ? "below" : "above",
      );
    };

    // Opening Range Breakout — 3 anchors: OR high, OR low, breakout
    // close. Render the high/low as a horizontal band anchored at the
    // session-open bar, and drop a triangle marker at the breakout
    // close bar in the variant's colour.
    const renderOrb = (d: ChartWorkspaceDetection) => {
      if (d.anchors.length < 3) return;
      const highAnchor = d.anchors.find(
        (a) => (a.label || a.label_override)?.toLowerCase().includes("high"),
      );
      const lowAnchor = d.anchors.find(
        (a) => (a.label || a.label_override)?.toLowerCase().includes("low"),
      );
      const breakAnchor = d.anchors.find(
        (a) => (a.label || a.label_override)?.toLowerCase().includes("break"),
      );
      if (!highAnchor || !lowAnchor || !breakAnchor) return;
      const t0 = anchorTime(highAnchor);
      const tBreak = anchorTime(breakAnchor);
      const hiP = parsePrice(highAnchor.price);
      const loP = parsePrice(lowAnchor.price);
      const brkP = parsePrice(breakAnchor.price);
      if (
        t0 === null ||
        tBreak === null ||
        Number.isNaN(hiP) ||
        Number.isNaN(loP) ||
        Number.isNaN(brkP)
      ) {
        return;
      }
      const variant = variantFromSubkind(d.subkind);
      const color = colorFor(variant);
      // Draw the OR band as a filled rectangle from session-open bar
      // to breakout bar. Emerald for bull, rose for bear, amber for
      // neutral. Alpha kept low so successive bands stack readably.
      const rect = new RectanglePrimitive({
        time1: t0,
        time2: tBreak,
        priceTop: Math.max(hiP, loP),
        priceBottom: Math.min(hiP, loP),
        fillColor: color + "1a",
        borderColor: color,
        borderWidth: 1,
      });
      candleSeries.attachPrimitive(rect);
      rectPrimitivesRef.current.push(rect);
      attachLabel(
        tBreak,
        brkP,
        labelFor(d),
        color,
        variant === "bear" ? "below" : "above",
      );
    };

    // Smart Money Concepts events — single-anchor markers (BOS/CHoCH/
    // MSS as short dashed horizontal lines at the structural price;
    // LiquiditySweep / FVI as dotted markers). The subkind prefix picks
    // the rendering kind so we don't dispatch on strings elsewhere.
    const renderSmc = (d: ChartWorkspaceDetection) => {
      if (d.anchors.length === 0) return;
      const a = d.anchors[0];
      const t = anchorTime(a);
      const p = parsePrice(a.price);
      if (t === null || Number.isNaN(p)) return;
      const variant = variantFromSubkind(d.subkind);
      const color = colorFor(variant);
      const kindPrefix = d.subkind.split("_")[0]; // bos / choch / mss / liquidity / fvi
      const isEvent = kindPrefix === "bos" || kindPrefix === "choch" || kindPrefix === "mss";
      const style = isEvent ? LineStyle.Dashed : LineStyle.Dotted;
      const s = chart.addSeries(LineSeries, {
        color,
        lineWidth: 1,
        lineStyle: style,
        priceLineVisible: false,
        lastValueVisible: false,
      });
      // Anchor a short horizontal segment (10 bars) so the event is
      // visible but doesn't overwhelm the chart. Forward projection
      // only — SMC levels signal future reversals, not past.
      const barIdx =
        typeof a.bar_index === "number" ? a.bar_index : candles.length - 1;
      const endIdx = Math.min(barIdx + 10, candles.length - 1);
      const tEnd = timeAt(endIdx);
      if (tEnd === null || (tEnd as number) <= (t as number)) return;
      s.setData([
        { time: t, value: p },
        { time: tEnd, value: p },
      ]);
      overlaySeriesRef.current.push(s);
      attachLabel(
        t,
        p,
        labelFor(d),
        color,
        variant === "bear" ? "below" : "above",
      );
    };

    // FAZ 25.4.A — Wyckoff event overlay. Each event paints a marker
    // at its trigger bar with a label that tells the reader WHAT
    // fired (SC, Spring, SOS, UTAD, etc.) and a colour that conveys
    // the institutional bias (bull = emerald, bear = rose). The
    // schematic phase (A-E) gets a small pill above the marker so
    // the operator sees where in the accumulation/distribution
    // sequence we are. Reference: docs/ELLIOTT_WYCKOFF_INTEGRATION.md
    // §II.1 — Spring + W2 / SOS + W3 / BC + W5 / UTAD + B / SOW + C
    // are the textbook entry windows.
    const wyckoffEventLabel = (subkind: string): string => {
      const head = subkind.replace(/_(bull|bear)$/i, "").toUpperCase();
      // Pretty-cased canonical names for the 12 events.
      switch (head) {
        case "SC":     return "SC";
        case "BC":     return "BC";
        case "AR":     return "AR";
        case "ST":     return "ST";
        case "SPRING": return "Spring";
        case "TEST":   return "Test";
        case "SOS":    return "SOS";
        case "SOW":    return "SOW";
        case "LPS":    return "LPS";
        case "PS":     return "PS";
        case "UTAD":   return "UTAD";
        case "BU":     return "BU";
        default:       return head;
      }
    };
    // Pre-compute price extents once so the cycle bands span the
    // visible price range. The bands are FULL-HEIGHT subtle backdrops
    // — they live behind everything else (range boxes, events,
    // candles).
    let cycleHi = Number.NEGATIVE_INFINITY;
    let cycleLo = Number.POSITIVE_INFINITY;
    for (const c of candles) {
      const h = typeof c.high === "number" ? c.high : Number(c.high);
      const l = typeof c.low === "number" ? c.low : Number(c.low);
      if (Number.isFinite(h) && h > cycleHi) cycleHi = h;
      if (Number.isFinite(l) && l < cycleLo) cycleLo = l;
    }
    if (!Number.isFinite(cycleHi) || !Number.isFinite(cycleLo)) {
      cycleHi = 1;
      cycleLo = 0;
    }
    // Pad ±5% so the band hugs the chart edges without clipping when
    // the user zooms vertically.
    const cyclePadding = (cycleHi - cycleLo) * 0.05;
    const cycleBandTop = cycleHi + cyclePadding;
    const cycleBandBottom = Math.max(cycleLo - cyclePadding, 0);

    const renderWyckoff = (d: ChartWorkspaceDetection) => {
      if (d.anchors.length === 0) return;
      // FAZ 25.4.D — four-phase macro market cycle bands
      // (Accumulation → Markup → Distribution → Markdown). Each cycle
      // segment paints a wide low-alpha backdrop covering the whole
      // visible price range for the duration of that phase. Subtle
      // enough to read through; the schematic range boxes (Accum/Dist)
      // sit ON TOP for the tighter framing.
      if (d.subkind.startsWith("cycle_") && d.anchors.length >= 2) {
        // FAZ 25.4.E — per-element Wyckoff filter gating.
        const cycleEnabled =
          (d.subkind === "cycle_markup" && wyckoffFilter.cycle_markup) ||
          (d.subkind === "cycle_distribution" && wyckoffFilter.cycle_distribution) ||
          (d.subkind === "cycle_markdown" && wyckoffFilter.cycle_markdown) ||
          (d.subkind === "cycle_accumulation" && wyckoffFilter.cycle_accumulation);
        if (!cycleEnabled) return;
        const sourceForFilter =
          typeof d.raw_meta?.source === "string"
            ? (d.raw_meta.source as string)
            : "event";
        const sourceEnabled =
          (sourceForFilter === "event" && wyckoffFilter.src_event) ||
          (sourceForFilter === "elliott" && wyckoffFilter.src_elliott) ||
          (sourceForFilter === "confluent" && wyckoffFilter.src_confluent);
        if (!sourceEnabled) return;
        const a0 = d.anchors[0];
        const a1 = d.anchors[1];
        const t0 = anchorTime(a0);
        const t1 = anchorTime(a1);
        if (t0 === null || t1 === null) return;
        // FAZ 25.4.E — color = ZigZag slot color (Z1..Z5 toolbar
        // palette), NOT phase color. Each Z degree's cycle paints in
        // its own ZigZag color so the operator can read at-a-glance
        // which Z's tilesheet they're looking at. Phase is conveyed by
        // the label text + position, not by hue.
        const phaseLabel: Record<string, string> = {
          cycle_accumulation: "Accumulation",
          cycle_markup:       "Markup",
          cycle_distribution: "Distribution",
          cycle_markdown:     "Markdown",
        };
        const label = phaseLabel[d.subkind];
        if (!label) return;
        // Slot comes back from the API as `pivot_level: "L0".."L5"`.
        // Parse the digit. Falls back to raw_meta.slot if pivot_level
        // is missing for any reason.
        const parseSlot = (): number => {
          if (typeof d.pivot_level === "string") {
            const m = d.pivot_level.match(/^L(\d+)$/);
            if (m) return parseInt(m[1], 10);
          }
          if (typeof d.raw_meta?.slot === "number") {
            return d.raw_meta.slot as number;
          }
          return 0;
        };
        const slotIdx = parseSlot();
        const slotColor =
          slots[slotIdx]?.color ?? DEFAULT_SLOTS[slotIdx]?.color ?? "#a78bfa";
        // Hex → "r,g,b" for rgba() string. Accepts "#RGB" / "#RRGGBB".
        const hexToRgb = (hex: string): string => {
          const h = hex.replace("#", "");
          const expand =
            h.length === 3 ? h.split("").map((c) => c + c).join("") : h;
          const r = parseInt(expand.slice(0, 2), 16);
          const g = parseInt(expand.slice(2, 4), 16);
          const b = parseInt(expand.slice(4, 6), 16);
          if ([r, g, b].some((n) => Number.isNaN(n))) return "167,139,250";
          return `${r},${g},${b}`;
        };
        const rgb = hexToRgb(slotColor);
        // Phase has two visual idioms because phases mean two
        // different things on a chart:
        //   - Accumulation / Distribution = SIDEWAYS RANGE → filled
        //     box reads as "price contained inside this rectangle"
        //   - Markup / Markdown = TREND LEG → top+bottom borders
        //     only (no fill) reads as "price travelled through this
        //     tunnel". A filled rectangle would dominate the chart
        //     because trend tiles span the whole motive (huge area)
        //     and any meaningful fill alpha makes candles unreadable.
        const isTrend =
          d.subkind === "cycle_markup" || d.subkind === "cycle_markdown";
        // Source-aware alpha hierarchy. Trend tiles use higher border
        // visibility + subtle fill so the channel reads through candles.
        const sourceRaw = d.raw_meta?.source;
        const source =
          typeof sourceRaw === "string" ? sourceRaw : "event";
        const styleBySource: Record<
          string,
          {
            rangeFill: number;
            rangeBorder: number;
            rangeWidth: 1 | 2 | 3;
            trendFill: number;
            trendBorder: number;
            trendWidth: 1 | 2 | 3;
          }
        > = {
          confluent: {
            rangeFill: 0.22, rangeBorder: 1.00, rangeWidth: 2,
            trendFill: 0.18, trendBorder: 1.00, trendWidth: 3,
          },
          elliott: {
            rangeFill: 0.16, rangeBorder: 0.95, rangeWidth: 2,
            trendFill: 0.13, trendBorder: 0.95, trendWidth: 2,
          },
          event: {
            rangeFill: 0.10, rangeBorder: 0.70, rangeWidth: 1,
            trendFill: 0.08, trendBorder: 0.70, trendWidth: 1,
          },
        };
        const styleSrc = styleBySource[source] ?? styleBySource.event;
        const fillAlpha = isTrend ? styleSrc.trendFill : styleSrc.rangeFill;
        const borderAlpha = isTrend
          ? styleSrc.trendBorder
          : styleSrc.rangeBorder;
        const borderWidth = isTrend
          ? styleSrc.trendWidth
          : styleSrc.rangeWidth;
        const fill = `rgba(${rgb},${fillAlpha})`;
        const border = `rgba(${rgb},${borderAlpha})`;
        // Use phase_high / phase_low from raw_meta when present so
        // the box hugs the actual price range of the phase rather than
        // spanning the entire visible chart. Falls back to full-height
        // backdrop when bounds are absent (legacy rows).
        const phaseHi =
          typeof d.raw_meta?.phase_high === "number"
            ? d.raw_meta.phase_high
            : null;
        const phaseLo =
          typeof d.raw_meta?.phase_low === "number"
            ? d.raw_meta.phase_low
            : null;
        const top = phaseHi !== null ? phaseHi : cycleBandTop;
        const bottom = phaseLo !== null ? phaseLo : cycleBandBottom;
        const rect = new RectanglePrimitive({
          time1: t0,
          time2: t1,
          priceTop: top,
          priceBottom: bottom,
          fillColor: fill,
          borderColor: border,
          borderWidth,
        });
        candleSeries.attachPrimitive(rect);
        rectPrimitivesRef.current.push(rect);
        // Label: phase name + Z-slot tag + source tag.
        // Confluent gets ★, Elliott ◆ for at-a-glance confidence read.
        const sourceSuffix =
          source === "confluent" ? " ★" : source === "elliott" ? " ◆" : "";
        const zTag = `Z${slotIdx + 1}`;
        attachLabel(
          t0,
          top,
          `${label} · ${zTag}${sourceSuffix}`,
          border,
          "above"
        );
        return;
      }
      // FAZ 25.4.B — schematic range boxes (Accumulation / Distribution
      // rectangles). Two anchors: [start_bar, range_low] and
      // [end_bar, range_high]. Tinted fill behind the bar tape so the
      // events read like annotations on a labelled box.
      if (d.subkind.startsWith("range_") && d.anchors.length >= 2) {
        if (!wyckoffFilter.ranges) return;
        const a0 = d.anchors[0];
        const a1 = d.anchors[1];
        const t0 = anchorTime(a0);
        const t1 = anchorTime(a1);
        const lo = parsePrice(a0.price);
        const hi = parsePrice(a1.price);
        if (
          t0 === null ||
          t1 === null ||
          Number.isNaN(lo) ||
          Number.isNaN(hi)
        ) {
          return;
        }
        const isAccum = d.subkind === "range_accumulation";
        // 18% alpha fill (was 8%) so the box reads cleanly against
        // candles + a 2px border for the outline. Distribution
        // boxes use rose; accumulation emerald — colour-codes the
        // bias at a glance without reading the label.
        const fill = isAccum ? "#22c55e2e" : "#ef44442e";
        const border = isAccum ? "#22c55e" : "#ef4444";
        const rect = new RectanglePrimitive({
          time1: t0,
          time2: t1,
          priceTop: hi,
          priceBottom: lo,
          fillColor: fill,
          borderColor: border,
          borderWidth: 2,
        });
        candleSeries.attachPrimitive(rect);
        rectPrimitivesRef.current.push(rect);
        const phaseRaw = d.raw_meta?.phase;
        const phase =
          typeof phaseRaw === "string" ? phaseRaw.toUpperCase() : "";
        // FAZ 25.4.E — schematic range label disambiguated from
        // macro cycle labels. Cycle tiles read "Distribution · Z5 ◆"
        // while schematic ranges read "Range Dist · PC". The "P{x}"
        // suffix is the inner Wyckoff schematic phase (A/B/C/D/E),
        // a different concept from the 4-phase macro cycle.
        const label = isAccum
          ? `Range Accum${phase ? ` · P${phase}` : ""}`
          : `Range Dist${phase ? ` · P${phase}` : ""}`;
        attachLabel(t0, hi, label, border, "above");
        return;
      }
      // FAZ 25.4.E — Wyckoff context filter (Gemini + Claude audit).
      // Spring is a Phase-C accumulation event ONLY; UTAD is a
      // Phase-C distribution event ONLY. Detector fires them on
      // local price wick + reclaim — without checking macro phase
      // context the same setup gets labelled in the WRONG cycle
      // (Spring inside Distribution box; UTAD inside Accumulation
      // box). Suppress the contextually-impossible labels.
      //
      // We've already built the auxDetections list which contains
      // the cycle_* tiles. Use them to look up the active phase at
      // the event's time and gate Spring/UTAD accordingly.
      const eventTs =
        typeof d.anchors[0]?.time === "string"
          ? Math.floor(new Date(d.anchors[0].time).getTime() / 1000)
          : NaN;
      const phaseAt = (ts: number): "accumulation" | "distribution" | "markup" | "markdown" | null => {
        if (!Number.isFinite(ts)) return null;
        for (const cyc of auxDetections) {
          if (cyc.family !== "wyckoff") continue;
          if (!cyc.subkind.startsWith("cycle_")) continue;
          const t0raw = cyc.anchors[0]?.time;
          const t1raw = cyc.anchors[cyc.anchors.length - 1]?.time;
          if (typeof t0raw !== "string" || typeof t1raw !== "string") continue;
          const t0 = Math.floor(new Date(t0raw).getTime() / 1000);
          const t1 = Math.floor(new Date(t1raw).getTime() / 1000);
          if (ts >= t0 && ts <= t1) {
            const phaseStr = cyc.subkind.replace("cycle_", "");
            if (
              phaseStr === "accumulation" ||
              phaseStr === "distribution" ||
              phaseStr === "markup" ||
              phaseStr === "markdown"
            ) {
              return phaseStr;
            }
          }
        }
        return null;
      };
      const ctxPhase = phaseAt(eventTs);
      // Spring belongs in Accumulation / Markup-launch zone.
      // Suppress Spring inside Distribution / Markdown context.
      if (
        d.subkind.startsWith("spring_") &&
        ctxPhase &&
        (ctxPhase === "distribution" || ctxPhase === "markdown")
      ) {
        return;
      }
      // UTAD belongs in Distribution / Markdown-launch zone.
      // Suppress UTAD inside Accumulation / Markup context.
      if (
        d.subkind.startsWith("utad_") &&
        ctxPhase &&
        (ctxPhase === "accumulation" || ctxPhase === "markup")
      ) {
        return;
      }

      // FAZ 25.4.E — per-event-group filter. Group the 12 Wyckoff
      // events into 4 toggleable buckets so the operator can quickly
      // mute the noisy ones (AR/ST/LPS/PS/BU/Test fire often).
      const eventGroup = (sk: string): keyof typeof wyckoffFilter | null => {
        if (sk.startsWith("sc_") || sk.startsWith("bc_"))
          return "events_climax";
        if (sk.startsWith("spring_") || sk.startsWith("utad_"))
          return "events_spring";
        if (sk.startsWith("sos_") || sk.startsWith("sow_"))
          return "events_sos_sow";
        if (
          sk.startsWith("ar_") ||
          sk.startsWith("st_") ||
          sk.startsWith("lps_") ||
          sk.startsWith("ps_") ||
          sk.startsWith("bu_") ||
          sk.startsWith("test_")
        )
          return "events_other";
        return null;
      };
      const grp = eventGroup(d.subkind);
      if (grp && !wyckoffFilter[grp]) return;
      const a = d.anchors[0];
      const t = anchorTime(a);
      const p = parsePrice(a.price);
      if (t === null || Number.isNaN(p)) return;
      const isBull = d.subkind.endsWith("_bull");
      const color = isBull ? "#22c55e" : "#ef4444";
      // Phase pill — read from the chart workspace detection's
      // raw_meta (the v2_chart endpoint passes raw_meta through).
      const phaseRaw = d.raw_meta?.phase;
      const phase = typeof phaseRaw === "string" ? phaseRaw.toUpperCase() : null;
      const baseLabel = wyckoffEventLabel(d.subkind);
      const fullLabel = phase ? `${baseLabel} · P${phase}` : baseLabel;
      // Position labels: bull events below the bar (the Spring's
      // wick is below; SOS launches up from a low), bear events
      // above (BC at the top, UTAD shakeouts above range).
      const position: "above" | "below" = isBull ? "below" : "above";
      attachLabel(t, p, fullLabel, color, position);
      // Short horizontal accent segment at the event price (5-bar
      // forward) so the marker has visible mass on the chart and the
      // reader can find it when scrolling horizontally. Mirrors the
      // SMC overlay pattern. lightweight-charts requires strictly
      // ascending timestamps; identical-time points trip the assert,
      // so we draw across bars rather than vertically.
      const barIdx =
        typeof a.bar_index === "number" ? a.bar_index : candles.length - 1;
      const tEnd = timeAt(Math.min(barIdx + 5, candles.length - 1));
      if (tEnd !== null && (tEnd as number) > (t as number)) {
        const s = chart.addSeries(LineSeries, {
          color,
          lineWidth: 1,
          lineStyle: LineStyle.Dotted,
          priceLineVisible: false,
          lastValueVisible: false,
        });
        s.setData([
          { time: t, value: p },
          { time: tEnd, value: p },
        ]);
        overlaySeriesRef.current.push(s);
      }
    };

    // FAZ 25.x — elliott_full overlay. Surfaces the previously-dormant
    // detector outputs (leading/ending diagonals, regular/expanded/
    // running flats, W1/W3/W5 extended impulses, truncated fifth,
    // W-X-Y combinations). Each subkind paints with a family-specific
    // color so the eye can separate diagonals (purple) from flats
    // (cyan) from combinations (rose) at a glance — same row layout
    // as classical so the reader doesn't have to learn a new affordance.
    const elliottFullColorFor = (subkind: string): string => {
      if (subkind.startsWith("leading_diagonal")) return "#a855f7"; // purple
      if (subkind.startsWith("ending_diagonal")) return "#7c3aed"; // violet
      if (subkind.startsWith("flat_regular")) return "#06b6d4"; // cyan
      if (subkind.startsWith("flat_expanded")) return "#0891b2"; // darker cyan
      if (subkind.startsWith("flat_running")) return "#0e7490"; // dark cyan
      if (subkind.startsWith("impulse_truncated")) return "#f97316"; // orange
      if (subkind.startsWith("impulse_w1_extended")) return "#84cc16"; // lime
      if (subkind.startsWith("impulse_w3_extended")) return "#65a30d"; // dark lime
      if (subkind.startsWith("impulse_w5_extended")) return "#4d7c0f"; // darkest lime
      if (subkind.startsWith("combination_wxy")) return "#ec4899"; // rose
      return "#9ca3af"; // grey fallback
    };
    const renderElliottFull = (d: ChartWorkspaceDetection) => {
      if (d.anchors.length < 2) return;
      const pts: LineData[] = d.anchors
        .map((a) => {
          const t = anchorTime(a);
          const p = parsePrice(a.price);
          return t !== null && !Number.isNaN(p) ? { time: t, value: p } : null;
        })
        .filter((x): x is LineData => x !== null);
      if (pts.length < 2) return;
      const clean = dedupeByTime(pts);
      if (clean.length < 2) return;
      const variant = variantFromSubkind(d.subkind);
      const color = elliottFullColorFor(d.subkind);
      const style = d.state === "invalidated" ? LineStyle.Dashed : LineStyle.Solid;
      const s = chart.addSeries(LineSeries, {
        color,
        lineWidth: 2,
        lineStyle: style,
        priceLineVisible: false,
        lastValueVisible: false,
      });
      s.setData(clean);
      overlaySeriesRef.current.push(s);
      // Anchor labels — every detector ships its own (W0..W5, A/B/C,
      // X, etc.) and we already mirror them as `label_override` on the
      // backend. Render them inline so the reader can read off
      // structure without having to inspect the row. Position is
      // derived from each anchor's price relative to the polyline
      // average so highs go above and lows go below regardless of
      // overall pattern direction.
      const avgPrice =
        clean.reduce((sum, c) => sum + c.value, 0) / clean.length;
      for (const a of d.anchors) {
        const t = anchorTime(a);
        const p = parsePrice(a.price);
        const label = a.label_override ?? a.label;
        if (t === null || Number.isNaN(p) || !label) continue;
        attachLabel(t, p, label, color, p >= avgPrice ? "above" : "below");
      }
      const tip = clean[clean.length - 1];
      attachLabel(
        tip.time,
        tip.value,
        labelFor(d),
        color,
        variant === "bull" ? "above" : "below",
      );
    };

    const renderByFamily: Record<string, (d: ChartWorkspaceDetection) => void> = {
      classical: renderClassical,
      range: renderRange,
      gap: renderGap,
      candle: renderCandle,
      orb: renderOrb,
      smc: renderSmc,
      elliott_full: renderElliottFull,
      wyckoff: renderWyckoff,
    };
    const familyEnabled = (f: string): boolean => {
      if (f === "classical") return showClassical;
      if (f === "range") return showRange;
      if (f === "gap") return showGap;
      if (f === "candle") return showCandles;
      if (f === "orb") return showOrb;
      if (f === "smc") return showSmc;
      if (f === "elliott_full") return showElliottFull;
      if (f === "wyckoff") return showWyckoff;
      return false;
    };

    // FAZ 25.4.B — Wyckoff render policy.
    //
    // Range boxes (subkind LIKE 'range_*') always render — they're
    // the schematic frame and there's at most one per (sym, tf, slot).
    //
    // Individual events: BC/SC/AR/ST/Spring/UTAD/SOS/SOW/LPS/BU/PS/Test
    // can fire repeatedly in a single range (UTAD especially — every
    // wick above range top qualifies). User reported visual overload
    // (\"UTAD-PC neden çok\") + missing BC/AR/ST overshadowed by the
    // repeated UTADs. Apply proximity dedup: per subkind, drop any
    // event whose bar is within `proximityBars` of an already-kept
    // event of the same subkind. Keeps the freshest per cluster, lets
    // every distinct event type surface.
    // FAZ 25.4.E — wider proximity dedup window + per-subkind cap.
    // User audit (Gemini + Claude both flagged): \"5+ UTAD/Test/SOS
    // labels stacking at the right edge, unreadable\". Wyckoff
    // schematic should fire each Phase-C event ONCE per range; our
    // detector currently fires at every minor wick + reclaim. Until
    // the backend Phase-C state machine ships, the chart-side
    // dedup needs to be MUCH stricter. 8-bar window was 32h on 4h —
    // not enough. 24 bars = 4 days, closer to what a real Phase C
    // event window looks like. Also cap each subkind to the top-N
    // strongest globally so the chart can't be drowned by one
    // event type even if many pass the dedup proximity.
    const proximityBars = 24;
    const maxPerSubkind = 3;
    const wyckoffEvents = auxDetections.filter((d) => {
      const family = d.family || d.kind;
      if (family !== "wyckoff") return false;
      // Range + cycle rows always pass — we filter only individual
      // events. Cycle bands are 4-per-rotation so dedup is unnecessary,
      // and ranges already are at most one per (sym, tf, slot).
      if (d.subkind.startsWith("range_")) return false;
      if (d.subkind.startsWith("cycle_")) return false;
      // FAZ 25.4.E — was filtering by `anchorBar < candles.length`
      // which DROPPED every Spring/UTAD/etc. event because writer
      // bar_index lives in a 2000-bar frame while the chart loads
      // 1500 candles at 4h. anchor.time is the invariant — let
      // anchorTime() handle out-of-range times naturally.
      return d.anchors.length > 0;
    });
    // Score-based dedup (user: \"utad-pc'lerden hangisini neye göre
    // tekilleştireceksin\"). Sort by `confidence` DESC — the
    // detector'\\''s own structural score (volume_ratio × range_ratio
    // × body coverage). Walk in that order; for each event, KEEP it
    // only if no already-kept event of the same subkind sits within
    // proximityBars. Result: the strongest UTAD/Spring/SOS in each
    // cluster wins — the one with the most informative volume +
    // range fingerprint, not just the latest tick.
    const scoreOf = (d: ChartWorkspaceDetection): number => {
      const c = d.confidence;
      if (typeof c === "number") return c;
      if (typeof c === "string") {
        const p = parseFloat(c);
        return Number.isFinite(p) ? p : 0;
      }
      return 0;
    };
    wyckoffEvents.sort((a, b) => scoreOf(b) - scoreOf(a));
    const keptByKind = new Map<string, number[]>();
    const wyckoffKeep = new Set<string>();
    // Estimate the bar duration in seconds so we can convert the
    // proximityBars threshold (in BARS) to a time delta. Falls back
    // to 4h = 14400s if we can't measure.
    let secsPerBar = 14400;
    if (candles.length >= 2) {
      const t0 = Math.floor(new Date(candles[0].time).getTime() / 1000);
      const t1 = Math.floor(new Date(candles[1].time).getTime() / 1000);
      const dt = Math.abs(t1 - t0);
      if (dt > 0) secsPerBar = dt;
    }
    const proximitySecs = proximityBars * secsPerBar;
    for (const det of wyckoffEvents) {
      const a = det.anchors[0];
      // FAZ 25.4.E — API DetectionAnchor only carries time/price/label
      // (no bar_index). Previous proximity dedup used `a.bar_index`
      // which was always undefined → bar=-1 → every event skipped →
      // wyckoffKeep empty → render loop drops every event. User
      // report: \"wyckoff eventleri seçili olduğu halde grafikte
      // işaretlenmemiş\". Switch to TIME-based proximity.
      const ts =
        typeof a?.time === "string"
          ? Math.floor(new Date(a.time).getTime() / 1000)
          : NaN;
      if (!Number.isFinite(ts)) continue;
      const list = keptByKind.get(det.subkind) ?? [];
      // Per-subkind global cap — even outside the proximity window,
      // never render more than `maxPerSubkind` total per chart.
      // wyckoffEvents is already sorted by score DESC so we keep
      // the strongest N.
      if (list.length >= maxPerSubkind) continue;
      const tooClose = list.some((b) => Math.abs(b - ts) < proximitySecs);
      if (!tooClose) {
        list.push(ts);
        keptByKind.set(det.subkind, list);
        wyckoffKeep.add(det.id);
      }
    }
    for (const det of auxDetections) {
      const family = det.family || det.kind;
      if (!familyEnabled(family)) continue;
      if (
        family === "wyckoff" &&
        !det.subkind.startsWith("range_") &&
        !det.subkind.startsWith("cycle_")
      ) {
        // bar_index check removed — see wyckoffEvents filter above.
        // The writer's bar_index frame (2000 bars) != chart's candles
        // frame (e.g. 1500 at 4h), so this gate was rejecting valid
        // events. anchorTime() handles visibility via anchor.time.
        if (!wyckoffKeep.has(det.id)) continue;
      }
      const render = renderByFamily[family];
      if (!render) continue;
      render(det);
    }

    // ── Technical indicator overlays ────────────────────────────────
    //
    // Each series map arrives as `{name → {sub → values[]}}` keyed by
    // indicator name. We convert the parallel-array form back to a
    // LineSeries via `candles[i].time` as the time axis. NaN gaps become
    // whitespace in the line. The price-pane overlays below are the
    // minimum-viable set (SuperTrend / Keltner / Ichimoku cloud /
    // Donchian / PSAR); oscillators (RSI, Williams%R, CMF, Aroon,
    // TTM Squeeze) move to a dedicated sub-pane in PR-11H alongside the
    // add/remove indicator panel.
    const indSeries = indicators.data?.series ?? {};
    // Candles arrive with ISO-8601 `time` strings from /v2/zigzag;
    // convert once to UTCTimestamp (seconds) so lightweight-charts
    // doesn't mistake them for daily business-day strings.
    const candleTimes: Time[] = candles.map(
      (c) => (Math.floor(new Date(c.time).getTime() / 1000) as UTCTimestamp) as Time,
    );
    const toLineData = (values: number[]): LineData[] => {
      const out: LineData[] = [];
      for (let i = 0; i < Math.min(values.length, candleTimes.length); i++) {
        const v = values[i];
        if (v === null || v === undefined || Number.isNaN(v)) continue;
        out.push({ time: candleTimes[i], value: v });
      }
      return dedupeByTime(out);
    };
    const addLine = (values: number[] | undefined, color: string, width = 1) => {
      if (!values) return;
      const pts = toLineData(values);
      if (pts.length < 2) return;
      const s = chart.addSeries(LineSeries, {
        color,
        lineWidth: width as 1 | 2,
        priceLineVisible: false,
        lastValueVisible: false,
      });
      s.setData(pts);
      overlaySeriesRef.current.push(s);
    };

    if (indSeries.supertrend) {
      const st = indSeries.supertrend;
      // Draw only the "active" band per bar: lower when trend==+1,
      // upper when trend==-1. Matches Pine's ta.supertrend visual.
      const trend = st.trend ?? [];
      const active: number[] = new Array(candleTimes.length).fill(Number.NaN);
      for (let i = 0; i < active.length; i++) {
        const t = trend[i] ?? 0;
        if (t > 0) active[i] = st.lower?.[i] ?? Number.NaN;
        else if (t < 0) active[i] = st.upper?.[i] ?? Number.NaN;
      }
      addLine(active, "#22c55e", 2);
    }
    if (indSeries.keltner) {
      addLine(indSeries.keltner.upper, "#eab308", 1);
      addLine(indSeries.keltner.mid, "#a16207", 1);
      addLine(indSeries.keltner.lower, "#eab308", 1);
    }
    if (indSeries.ichimoku) {
      addLine(indSeries.ichimoku.tenkan, "#3b82f6", 1);
      addLine(indSeries.ichimoku.kijun, "#ef4444", 1);
      // Senkou A & B outline the cloud — two thin lines, no fill
      // primitive for now (cloud fill lands with the area-band
      // primitive work in PR-11H).
      addLine(indSeries.ichimoku.senkou_a, "#22c55e", 1);
      addLine(indSeries.ichimoku.senkou_b, "#ef4444", 1);
      addLine(indSeries.ichimoku.chikou, "#a855f7", 1);
    }
    if (indSeries.donchian) {
      addLine(indSeries.donchian.upper, "#06b6d4", 1);
      addLine(indSeries.donchian.mid, "#0891b2", 1);
      addLine(indSeries.donchian.lower, "#06b6d4", 1);
    }
    if (indSeries.psar) {
      // SAR renders as a sparse dotted series — use LineStyle.Dotted
      // so gaps between dots stay visible on low-density timeframes.
      const pts = toLineData(indSeries.psar.sar ?? []);
      if (pts.length >= 2) {
        const s = chart.addSeries(LineSeries, {
          color: "#f59e0b",
          lineWidth: 1,
          lineStyle: LineStyle.Dotted,
          priceLineVisible: false,
          lastValueVisible: false,
        });
        s.setData(pts);
        overlaySeriesRef.current.push(s);
      }
    }

    // ── Oscillator pane (PR-11H) ───────────────────────────────────
    //
    // RSI / Williams %R / CMF / Aroon-oscillator / TTM Squeeze render
    // in a dedicated sub-pane (paneIndex=1) below the main price pane.
    // lightweight-charts v5 creates the pane on demand from the first
    // series; threshold lines (overbought/oversold) are attached via
    // `createPriceLine` on each oscillator's series so they scale with
    // zoom.
    const addOscLine = (values: number[] | undefined, color: string, width = 1) => {
      if (!values) return null;
      const pts = toLineData(values);
      if (pts.length < 2) return null;
      const s = chart.addSeries(
        LineSeries,
        {
          color,
          lineWidth: width as 1 | 2,
          priceLineVisible: false,
          lastValueVisible: true,
        },
        1, // paneIndex — oscillator pane
      );
      s.setData(pts);
      overlaySeriesRef.current.push(s);
      return s;
    };
    if (indSeries.rsi) {
      const s = addOscLine(indSeries.rsi.rsi, "#a855f7", 2);
      if (s) {
        s.createPriceLine({
          price: 70,
          color: "#ef4444",
          lineWidth: 1,
          lineStyle: LineStyle.Dashed,
          axisLabelVisible: true,
          title: "70",
        });
        s.createPriceLine({
          price: 30,
          color: "#22c55e",
          lineWidth: 1,
          lineStyle: LineStyle.Dashed,
          axisLabelVisible: true,
          title: "30",
        });
      }
    }
    if (indSeries.williams_r) {
      const s = addOscLine(indSeries.williams_r.williams_r, "#06b6d4", 2);
      if (s) {
        s.createPriceLine({
          price: -20,
          color: "#ef4444",
          lineWidth: 1,
          lineStyle: LineStyle.Dashed,
          axisLabelVisible: true,
          title: "-20",
        });
        s.createPriceLine({
          price: -80,
          color: "#22c55e",
          lineWidth: 1,
          lineStyle: LineStyle.Dashed,
          axisLabelVisible: true,
          title: "-80",
        });
      }
    }
    if (indSeries.cmf) {
      const s = addOscLine(indSeries.cmf.cmf, "#f59e0b", 2);
      if (s) {
        s.createPriceLine({
          price: 0,
          color: "#71717a",
          lineWidth: 1,
          lineStyle: LineStyle.Dotted,
          axisLabelVisible: false,
          title: "",
        });
      }
    }
    if (indSeries.aroon) {
      // Aroon oscillator = up - down, scaled [-100, 100].
      addOscLine(indSeries.aroon.osc, "#ec4899", 2);
    }
    if (indSeries.ttm_squeeze) {
      // TTM Squeeze is boolean (0/1) — rendered as a thin flag at the
      // bottom of the oscillator pane. Scale different from RSI so
      // it lands on its own right-side axis.
      const vals = (indSeries.ttm_squeeze.squeeze ?? []).map((x) =>
        x > 0.5 ? 0.95 : 0.05,
      );
      addOscLine(vals, "#f472b6", 1);
    }
    if (indSeries.macd) {
      // MACD has 3 series: line (blue), signal (orange), histogram
      // (filled area). Histogram > 0 = bullish momentum, < 0 = bear.
      // Render line + signal as thin lines and histogram as a thicker
      // bar-like polyline anchored at zero.
      addOscLine(indSeries.macd.macd, "#3b82f6", 2);
      addOscLine(indSeries.macd.signal, "#f97316", 1);
      // Histogram: render as a colored line for now (fill primitive
      // pending). Positive ticks emerald, negative rose.
      const hist = indSeries.macd.hist ?? [];
      const histPos = hist.map((x) => (x > 0 ? x : Number.NaN));
      const histNeg = hist.map((x) => (x < 0 ? x : Number.NaN));
      addOscLine(histPos, "#22c55e", 1);
      addOscLine(histNeg, "#ef4444", 1);
    }
    if (indSeries.stochastic) {
      // Stochastic %K (cyan) + %D (orange) — bounded 0..100.
      addOscLine(indSeries.stochastic.k, "#06b6d4", 2);
      addOscLine(indSeries.stochastic.d, "#f59e0b", 1);
    }
    if (indSeries.obv) {
      // OBV is unbounded cumulative — different scale from RSI (0..100).
      // Add as its own line; lightweight-charts auto-scales the pane.
      addOscLine(indSeries.obv.obv, "#a855f7", 2);
    }
    if (indSeries.atr) {
      // ATR — volatility line, unbounded positive.
      addOscLine(indSeries.atr.atr, "#eab308", 2);
    }
    if (indSeries.bollinger) {
      // Bollinger bands render in the PRICE pane, not osc. Use the
      // same addLine helper as supertrend/keltner above.
      addLine(indSeries.bollinger.upper, "#a78bfa", 1);
      addLine(indSeries.bollinger.mid, "#71717a", 1);
      addLine(indSeries.bollinger.lower, "#a78bfa", 1);
    }

    // ── IQ price-line overlays (FAZ 25 PR-25C / PR-25D) ─────────────
    // Caller (IQChart) passes a flat list of {price, color, title}
    // tuples — one per entry / SL / TP level across all active iq_d
    // and iq_t setups for this symbol+tf. We attach each as a
    // horizontal price line on the candle series so it scales with
    // price axis automatically and follows zoom/pan natively.
    const overlays = defaults?.priceLineOverlays ?? [];
    for (const o of overlays) {
      if (!Number.isFinite(o.price) || o.price <= 0) continue;
      const styleEnum =
        o.lineStyle === "dashed"
          ? LineStyle.Dashed
          : o.lineStyle === "dotted"
          ? LineStyle.Dotted
          : LineStyle.Solid;
      try {
        const handle = candleSeries.createPriceLine({
          price: o.price,
          color: o.color,
          lineWidth: (o.lineWidth ?? 1) as 1 | 2 | 3 | 4,
          lineStyle: styleEnum,
          axisLabelVisible: true,
          title: o.title,
        });
        priceLineHandlesRef.current.push(handle);
      } catch {
        /* createPriceLine on a freshly-disposed chart can throw */
      }
    }
  }, [
    data.data, pineOutput, slots,
    showFibBand, showHhLl, onlyLatestMotive, showZigzag, showElliott, fibExtend,
    showElliottEarly, earlyMarkers,
    showImpulse, showZigzagAbc,
    showFlatRegular, showFlatExpanded, showFlatRunning,
    showTriContracting, showTriExpanding, showTriBarrier,
    showHarmonic, harmonicOutput,
    harmonicFilters, showHarmonicTargets,
    auxDetections, showClassical, showRange, showGap, showCandles, showOrb, showSmc,
    showElliottFull, showWyckoff, wyckoffFilter, detectionSource, enabledSlotIndices,
    indicators.data, showSuperTrend, showKeltner, showIchimoku, showDonchian, showPsar,
    showRsi, showWilliamsR, showCmf, showAroon, showTtmSqueeze,
    showMacd, showStochastic, showObv, showAtr, showBollinger,
    defaults?.priceLineOverlays,
  ]);

  const venueList: VenueOpt[] = venues.data ?? [];
  const venueOpt = useMemo(
    () => venueList.find((v) => v.exchange === exchange && v.segment === segment),
    [venueList, exchange, segment]
  );
  const updateSlot = (idx: number, patch: Partial<LevelSlot>) => {
    setSlots((prev) => prev.map((s, i) => (i === idx ? { ...s, ...patch } : s)));
  };
  const totalMotives = pineOutput
    ? pineOutput.levels.reduce((s, l) => s + l.motives.length, 0)
    : 0;

  return (
    <div
      className={
        defaults?.embedded
          ? "flex h-full min-h-0 flex-col gap-1 p-1"
          : "-m-6 flex h-[calc(100vh-57px)] flex-col gap-1 p-1"
      }
    >
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <select
          className="rounded bg-zinc-900 px-2 py-1"
          value={`${exchange}:${segment}`}
          onChange={(e) => {
            const [ex, sg] = e.target.value.split(":");
            setExchange(ex); setSegment(sg);
          }}
        >
          {venueList.map((v) => (
            <option key={`${v.exchange}:${v.segment}`} value={`${v.exchange}:${v.segment}`}>
              {v.exchange} · {v.segment}
            </option>
          ))}
        </select>
        <select
          className="rounded bg-zinc-900 px-2 py-1"
          value={symbol}
          onChange={(e) => setSymbol(e.target.value)}
        >
          {(venueOpt?.symbols ?? [symbol]).map((s) => (
            <option key={s} value={s}>{s}</option>
          ))}
        </select>
        <div className="flex gap-1">
          {TIMEFRAMES.map((t) => (
            <button
              key={t}
              onClick={() => setTf(t)}
              className={`rounded px-2 py-1 text-xs ${tf === t ? "bg-zinc-700 text-white" : "bg-zinc-900 text-zinc-400"}`}
            >
              {t}
            </button>
          ))}
        </div>
        <div className="ml-4 flex flex-wrap items-center gap-3">
          {slots.map((slot, idx) => (
            <div key={idx} className="flex items-center gap-1 rounded bg-zinc-900 px-2 py-1 text-xs">
              <input
                type="checkbox"
                checked={slot.enabled}
                onChange={(e) => updateSlot(idx, { enabled: e.target.checked })}
              />
              <span className="font-mono text-zinc-400">{`Z${idx + 1}`}</span>
              <input
                type="number" min={1} max={256} step={1}
                value={slot.length}
                onChange={(e) => {
                  const n = Math.max(1, Math.min(256, Number(e.target.value) || 1));
                  updateSlot(idx, { length: n });
                }}
                className="w-14 rounded bg-zinc-800 px-1 text-right"
              />
              <input
                type="color"
                value={slot.color}
                onChange={(e) => updateSlot(idx, { color: e.target.value })}
                className="h-5 w-5 cursor-pointer rounded border-none bg-transparent p-0"
              />
            </div>
          ))}
          <label className="flex cursor-pointer items-center gap-1 text-xs">
            <input type="checkbox" checked={showZigzag} onChange={(e) => setShowZigzag(e.target.checked)} />
            Zigzag
          </label>
          <label className="flex cursor-pointer items-center gap-1 text-xs">
            <input type="checkbox" checked={showElliott} onChange={(e) => setShowElliott(e.target.checked)} />
            Elliott formations
          </label>
          <label
            className="flex cursor-pointer items-center gap-1 text-xs"
            title="FAZ 25 PR-25A — nascent (N), forming (F), extended (X) impulse markers from elliott_early"
          >
            <input
              type="checkbox"
              checked={showElliottEarly}
              onChange={(e) => setShowElliottEarly(e.target.checked)}
            />
            <span className="text-emerald-400">N/F/X early</span>
          </label>
          <label className="flex cursor-pointer items-center gap-1 text-xs">
            <input type="checkbox" checked={showFibBand} onChange={(e) => setShowFibBand(e.target.checked)} />
            Fib band
          </label>
          <label className="flex cursor-pointer items-center gap-1 text-xs">
            <input type="checkbox" checked={fibExtend} onChange={(e) => setFibExtend(e.target.checked)} />
            Fib extend
          </label>
          <label className="flex cursor-pointer items-center gap-1 text-xs">
            <input type="checkbox" checked={showHhLl} onChange={(e) => setShowHhLl(e.target.checked)} />
            HH/HL/LL/LH
          </label>
          <label className="flex cursor-pointer items-center gap-1 text-xs">
            <input type="checkbox" checked={onlyLatestMotive} onChange={(e) => setOnlyLatestMotive(e.target.checked)} />
            Only latest motive
          </label>
          {/* Elliott source: live-compute vs DB-read (persisted detections). */}
          <div className="ml-2 flex items-center gap-1 text-xs">
            <span className="font-mono text-[10px] uppercase tracking-wider text-zinc-500">source</span>
            <button
              type="button"
              className={`rounded px-2 py-0.5 ${detectionSource === "live" ? "bg-emerald-600 text-white" : "bg-zinc-800 text-zinc-300"}`}
              onClick={() => setDetectionSource("live")}
            >
              live
            </button>
            <button
              type="button"
              className={`rounded px-2 py-0.5 ${detectionSource === "db" ? "bg-emerald-600 text-white" : "bg-zinc-800 text-zinc-300"}`}
              onClick={() => setDetectionSource("db")}
            >
              db
            </button>
          </div>
          {/* Run mode filter: the `detections` table carries a `mode`
              column ("live" | "dry" | "backtest"); each writer tags its
              rows by the runtime it executed under. Switching here
              flips the /v2/chart query so the overlay redraws with the
              matching mode's rows only. `dry` / `backtest` are wired
              end-to-end but currently empty — they populate as the
              dry-run and backtest engines come online (Faz 13+). */}
          <div className="ml-2 flex items-center gap-1 text-xs">
            <span className="font-mono text-[10px] uppercase tracking-wider text-zinc-500">mode</span>
            {(["live", "dry", "backtest"] as const).map((m) => (
              <button
                key={m}
                type="button"
                className={`rounded px-2 py-0.5 ${modeFilter === m ? "bg-sky-600 text-white" : "bg-zinc-800 text-zinc-300"}`}
                onClick={() => setModeFilter(m)}
                title={
                  m === "live"
                    ? "Canlı pazar verisi + gerçek emir detection'ları"
                    : m === "dry"
                      ? "Canlı veri + simüle emir (kağıt ticaret) detection'ları"
                      : "Tarihsel veri + backtest motoru detection'ları"
                }
              >
                {m}
              </button>
            ))}
          </div>
          {/* Auxiliary detector families (Classical / Range / Gap) —
              gated independently so traders can isolate pattern types
              without losing the Elliott/Harmonic overlay. */}
          <div className="ml-2 flex items-center gap-1 text-xs">
            <label className="flex cursor-pointer items-center gap-1">
              <input
                type="checkbox"
                checked={showClassical}
                onChange={(e) => setShowClassical(e.target.checked)}
              />
              Classical
            </label>
            <label className="flex cursor-pointer items-center gap-1">
              <input
                type="checkbox"
                checked={showRange}
                onChange={(e) => setShowRange(e.target.checked)}
              />
              Range
            </label>
            <label className="flex cursor-pointer items-center gap-1">
              <input
                type="checkbox"
                checked={showGap}
                onChange={(e) => setShowGap(e.target.checked)}
              />
              Gap
            </label>
            <label className="flex cursor-pointer items-center gap-1">
              <input
                type="checkbox"
                checked={showCandles}
                onChange={(e) => setShowCandles(e.target.checked)}
              />
              Candles
            </label>
            <label
              className="flex cursor-pointer items-center gap-1"
              title="Opening Range Breakout (Asia / London / NY session opens)"
            >
              <input
                type="checkbox"
                checked={showOrb}
                onChange={(e) => setShowOrb(e.target.checked)}
              />
              ORB
            </label>
            <label
              className="flex cursor-pointer items-center gap-1"
              title="Smart Money Concepts: BOS / CHoCH / MSS / Sweep / FVI"
            >
              <input
                type="checkbox"
                checked={showSmc}
                onChange={(e) => setShowSmc(e.target.checked)}
              />
              SMC
            </label>
            <label
              className="flex cursor-pointer items-center gap-1"
              title="Elliott full set: leading/ending diagonal, regular/expanded/running flat, W1/W3/W5 extended impulse, truncated fifth, W-X-Y combination"
            >
              <input
                type="checkbox"
                checked={showElliottFull}
                onChange={(e) => setShowElliottFull(e.target.checked)}
              />
              Elliott+
            </label>
            <label
              className="flex cursor-pointer items-center gap-1"
              title="Wyckoff: 12 events + ranges + 4-phase macro cycle. Click ⚙ to filter."
            >
              <input
                type="checkbox"
                checked={showWyckoff}
                onChange={(e) => setShowWyckoff(e.target.checked)}
              />
              Wyckoff
            </label>
            {showWyckoff && (
              <button
                type="button"
                className="ml-1 rounded border border-zinc-700 px-1 text-[10px] text-zinc-300 hover:bg-zinc-800"
                onClick={() => setWyckoffFilterOpen((v) => !v)}
                title="Wyckoff alt-filtre paneli (kutu/event/source aç-kapat)"
              >
                ⚙ {wyckoffFilterOpen ? "▼" : "▶"}
              </button>
            )}
          </div>
          {showWyckoff && wyckoffFilterOpen && (
            <div className="ml-2 mt-1 flex flex-wrap items-center gap-3 rounded border border-zinc-800 bg-zinc-950/60 px-2 py-1 text-[11px]">
              <span className="font-mono text-[10px] uppercase tracking-wider text-zinc-500">
                cycles
              </span>
              <label className="flex cursor-pointer items-center gap-1">
                <input
                  type="checkbox"
                  checked={wyckoffFilter.cycle_accumulation}
                  onChange={(e) =>
                    setWyckoffFilter((f) => ({ ...f, cycle_accumulation: e.target.checked }))
                  }
                />
                Accum
              </label>
              <label className="flex cursor-pointer items-center gap-1">
                <input
                  type="checkbox"
                  checked={wyckoffFilter.cycle_markup}
                  onChange={(e) =>
                    setWyckoffFilter((f) => ({ ...f, cycle_markup: e.target.checked }))
                  }
                />
                Markup
              </label>
              <label className="flex cursor-pointer items-center gap-1">
                <input
                  type="checkbox"
                  checked={wyckoffFilter.cycle_distribution}
                  onChange={(e) =>
                    setWyckoffFilter((f) => ({ ...f, cycle_distribution: e.target.checked }))
                  }
                />
                Dist
              </label>
              <label className="flex cursor-pointer items-center gap-1">
                <input
                  type="checkbox"
                  checked={wyckoffFilter.cycle_markdown}
                  onChange={(e) =>
                    setWyckoffFilter((f) => ({ ...f, cycle_markdown: e.target.checked }))
                  }
                />
                Markdown
              </label>
              <span className="ml-2 font-mono text-[10px] uppercase tracking-wider text-zinc-500">
                ranges
              </span>
              <label className="flex cursor-pointer items-center gap-1">
                <input
                  type="checkbox"
                  checked={wyckoffFilter.ranges}
                  onChange={(e) =>
                    setWyckoffFilter((f) => ({ ...f, ranges: e.target.checked }))
                  }
                />
                A-E box
              </label>
              <span className="ml-2 font-mono text-[10px] uppercase tracking-wider text-zinc-500">
                events
              </span>
              <label className="flex cursor-pointer items-center gap-1" title="SC + BC">
                <input
                  type="checkbox"
                  checked={wyckoffFilter.events_climax}
                  onChange={(e) =>
                    setWyckoffFilter((f) => ({ ...f, events_climax: e.target.checked }))
                  }
                />
                Climax
              </label>
              <label className="flex cursor-pointer items-center gap-1" title="Spring + UTAD (Phase-C high-conviction)">
                <input
                  type="checkbox"
                  checked={wyckoffFilter.events_spring}
                  onChange={(e) =>
                    setWyckoffFilter((f) => ({ ...f, events_spring: e.target.checked }))
                  }
                />
                Spring/UTAD
              </label>
              <label className="flex cursor-pointer items-center gap-1" title="SOS + SOW">
                <input
                  type="checkbox"
                  checked={wyckoffFilter.events_sos_sow}
                  onChange={(e) =>
                    setWyckoffFilter((f) => ({ ...f, events_sos_sow: e.target.checked }))
                  }
                />
                SOS/SOW
              </label>
              <label className="flex cursor-pointer items-center gap-1" title="AR / ST / LPS / PS / BU / Test">
                <input
                  type="checkbox"
                  checked={wyckoffFilter.events_other}
                  onChange={(e) =>
                    setWyckoffFilter((f) => ({ ...f, events_other: e.target.checked }))
                  }
                />
                Other
              </label>
              <span className="ml-2 font-mono text-[10px] uppercase tracking-wider text-zinc-500">
                source
              </span>
              <label className="flex cursor-pointer items-center gap-1" title="Confluent (event ∩ Elliott — highest confidence)">
                <input
                  type="checkbox"
                  checked={wyckoffFilter.src_confluent}
                  onChange={(e) =>
                    setWyckoffFilter((f) => ({ ...f, src_confluent: e.target.checked }))
                  }
                />
                ★
              </label>
              <label className="flex cursor-pointer items-center gap-1" title="Elliott-anchored (Pruden mapping)">
                <input
                  type="checkbox"
                  checked={wyckoffFilter.src_elliott}
                  onChange={(e) =>
                    setWyckoffFilter((f) => ({ ...f, src_elliott: e.target.checked }))
                  }
                />
                ◆
              </label>
              <label className="flex cursor-pointer items-center gap-1" title="Event-driven (climax-anchored)">
                <input
                  type="checkbox"
                  checked={wyckoffFilter.src_event}
                  onChange={(e) =>
                    setWyckoffFilter((f) => ({ ...f, src_event: e.target.checked }))
                  }
                />
                event
              </label>
            </div>
          )}
          {/* Technical indicator overlays. Price-pane overlays only —
              oscillator pane lands in PR-11H. */}
          <div className="ml-2 flex items-center gap-1 text-xs">
            <span className="font-mono text-[10px] uppercase tracking-wider text-zinc-500">
              ind
            </span>
            {(
              [
                ["supertrend", "SuperTrend", showSuperTrend, setShowSuperTrend],
                ["keltner", "Keltner", showKeltner, setShowKeltner],
                ["ichimoku", "Ichimoku", showIchimoku, setShowIchimoku],
                ["donchian", "Donchian", showDonchian, setShowDonchian],
                ["psar", "PSAR", showPsar, setShowPsar],
              ] as const
            ).map(([key, label, val, set]) => (
              <button
                key={key}
                type="button"
                className={`rounded px-2 py-0.5 ${val ? "bg-emerald-600 text-white" : "bg-zinc-800 text-zinc-300"}`}
                onClick={() => (set as (v: boolean) => void)(!val)}
              >
                {label}
              </button>
            ))}
          </div>
          {/* Oscillator pane (PR-11H) — separate strip below the price
              pane. Buttons are sky-coloured to distinguish from the
              price-pane overlays above. */}
          <div className="ml-2 flex items-center gap-1 text-xs">
            <span className="font-mono text-[10px] uppercase tracking-wider text-zinc-500">
              osc
            </span>
            {(
              [
                ["rsi", "RSI", showRsi, setShowRsi],
                ["macd", "MACD", showMacd, setShowMacd],
                ["stochastic", "Stoch", showStochastic, setShowStochastic],
                ["williams_r", "Williams %R", showWilliamsR, setShowWilliamsR],
                ["cmf", "CMF", showCmf, setShowCmf],
                ["aroon", "Aroon", showAroon, setShowAroon],
                ["ttm_squeeze", "TTM Sq", showTtmSqueeze, setShowTtmSqueeze],
                ["obv", "OBV", showObv, setShowObv],
                ["atr", "ATR", showAtr, setShowAtr],
                ["bollinger", "BB", showBollinger, setShowBollinger],
              ] as const
            ).map(([key, label, val, set]) => (
              <button
                key={key}
                type="button"
                className={`rounded px-2 py-0.5 ${val ? "bg-sky-600 text-white" : "bg-zinc-800 text-zinc-300"}`}
                onClick={() => (set as (v: boolean) => void)(!val)}
              >
                {label}
              </button>
            ))}
          </div>
          <button
            type="button"
            onClick={() => setShowFilters((f) => !f)}
            className={`ml-2 rounded border px-2 py-0.5 text-xs ${showFilters ? "border-emerald-600 bg-emerald-600/20 text-emerald-300" : "border-zinc-700 bg-zinc-900 text-zinc-300"}`}
            title="Elliott + Harmonic per-pattern filters"
          >
            {showFilters ? "▾ Filters" : "▸ Filters"}
          </button>
          {data.data && (
            <span className="ml-auto font-mono text-[11px] text-zinc-500">
              {data.isFetching ? "⟳ " : ""}{data.data.candles.length} candles · {totalMotives} motive
            </span>
          )}
        </div>
        {/* ── Elliott formations group — motive + corrective subtypes ── */}
        {showFilters && (
        <>
        <div className="flex w-full flex-wrap items-center gap-2 rounded border border-zinc-800 bg-zinc-950/40 p-2 text-xs">
          <span className="font-mono text-[10px] uppercase tracking-wider text-emerald-500">elliott</span>
          <label className="flex cursor-pointer items-center gap-1">
            <input type="checkbox" checked={showElliott} onChange={(e) => setShowElliott(e.target.checked)} />
            <span className="font-mono text-[10px] uppercase text-zinc-400">master</span>
          </label>
          <span className="text-zinc-700">·</span>
          <label className="flex cursor-pointer items-center gap-1">
            <input type="checkbox" checked={showImpulse} onChange={(e) => setShowImpulse(e.target.checked)} />
            Impulse
          </label>
          <label className="flex cursor-pointer items-center gap-1">
            <input type="checkbox" checked={showZigzagAbc} onChange={(e) => setShowZigzagAbc(e.target.checked)} />
            Zigzag (ABC)
          </label>
          <span className="text-zinc-700">·</span>
          <label className="flex cursor-pointer items-center gap-1">
            <input type="checkbox" checked={showFlatRegular} onChange={(e) => setShowFlatRegular(e.target.checked)} />
            Flat regular
          </label>
          <label className="flex cursor-pointer items-center gap-1">
            <input type="checkbox" checked={showFlatExpanded} onChange={(e) => setShowFlatExpanded(e.target.checked)} />
            Flat expanded
          </label>
          <label className="flex cursor-pointer items-center gap-1">
            <input type="checkbox" checked={showFlatRunning} onChange={(e) => setShowFlatRunning(e.target.checked)} />
            Flat running
          </label>
          <span className="text-zinc-700">·</span>
          <label className="flex cursor-pointer items-center gap-1">
            <input type="checkbox" checked={showTriContracting} onChange={(e) => setShowTriContracting(e.target.checked)} />
            Triangle contracting
          </label>
          <label className="flex cursor-pointer items-center gap-1">
            <input type="checkbox" checked={showTriExpanding} onChange={(e) => setShowTriExpanding(e.target.checked)} />
            Triangle expanding
          </label>
          <label className="flex cursor-pointer items-center gap-1">
            <input type="checkbox" checked={showTriBarrier} onChange={(e) => setShowTriBarrier(e.target.checked)} />
            Triangle barrier
          </label>
          {pineOutput && (
            <span className="ml-auto font-mono text-[11px] text-zinc-400">
              {totalMotives} motive
            </span>
          )}
        </div>

        {/* ── Harmonic formations group — per-pattern toggles + targets ── */}
        <div className="flex w-full flex-wrap items-center gap-2 rounded border border-zinc-800 bg-zinc-950/40 p-2 text-xs">
          <span className="font-mono text-[10px] uppercase tracking-wider text-fuchsia-500">harmonic</span>
          <label className="flex cursor-pointer items-center gap-1">
            <input type="checkbox" checked={showHarmonic} onChange={(e) => setShowHarmonic(e.target.checked)} />
            <span className="font-mono text-[10px] uppercase text-zinc-400">master</span>
          </label>
          <span className="text-zinc-700">·</span>
          {HARMONIC_KINDS.map(({ key, label }) => (
            <label key={key} className="flex cursor-pointer items-center gap-1">
              <input
                type="checkbox"
                checked={harmonicFilters[key] ?? true}
                onChange={(e) =>
                  setHarmonicFilters((prev) => ({ ...prev, [key]: e.target.checked }))
                }
              />
              {label}
            </label>
          ))}
          <span className="text-zinc-700">·</span>
          <label className="flex cursor-pointer items-center gap-1">
            <input
              type="checkbox"
              checked={showHarmonicTargets}
              onChange={(e) => setShowHarmonicTargets(e.target.checked)}
            />
            T1/T2/T3 targets
          </label>
          {showHarmonic && harmonicOutput && (
            <span className="ml-auto font-mono text-[11px] text-zinc-400">
              {harmonicOutput.patterns.length} pattern(s)
            </span>
          )}
        </div>
        </>
        )}
      </div>

      <div ref={containerRef} className="min-h-0 flex-1 rounded border border-zinc-800" />
    </div>
  );
}
