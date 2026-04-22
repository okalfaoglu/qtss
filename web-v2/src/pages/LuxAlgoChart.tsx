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

export function LuxAlgoChart() {
  const [exchange, setExchange] = useState("binance");
  const [segment, setSegment] = useState("futures");
  const [symbol, setSymbol] = useState("BTCUSDT");
  const [tf, setTf] = useState("4h");
  const [slots, setSlots] = useState<LevelSlot[]>(DEFAULT_SLOTS);
  const [showFibBand, setShowFibBand] = useState(true);
  const [showHhLl, setShowHhLl] = useState(false);
  const [onlyLatestMotive, setOnlyLatestMotive] = useState(true);
  const [showZigzag, setShowZigzag] = useState(true);
  const [showElliott, setShowElliott] = useState(true);
  const [fibExtend, setFibExtend] = useState(false);
  const [barLimit, setBarLimit] = useState(1000);

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
  const [showHarmonic, setShowHarmonic] = useState(true);

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

  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const candleSeriesRef = useRef<ISeriesApi<"Candlestick"> | null>(null);
  const overlaySeriesRef = useRef<ISeriesApi<"Line">[]>([]);
  const labelPrimitivesRef = useRef<TextLabelPrimitive[]>([]);
  const rectPrimitivesRef = useRef<RectanglePrimitive[]>([]);
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

  // Pan-left: bump limit when visible range nears left edge.
  const loadOlder = useCallback(() => {
    setBarLimit((n) => Math.min(n + 500, 5000));
  }, []);
  useEffect(() => {
    const chart = chartRef.current;
    if (!chart) return;
    const handler = (range: { from: number; to: number } | null) => {
      if (!range) return;
      if (range.from < 20) loadOlder();
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

    const candles = data.data.candles;
    const candleData: CandlestickData[] = candles.map((c) => ({
      time: Math.floor(new Date(c.time).getTime() / 1000) as Time,
      open: Number(c.open), high: Number(c.high), low: Number(c.low), close: Number(c.close),
    }));
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
    for (const level of pineOutput.levels) {
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
          const pts: LineData[] = mw.anchors
            .map((a) => {
              const t = timeAt(a.bar_index);
              return t === null ? null : { time: t, value: a.price };
            })
            .filter((x): x is LineData => x !== null);
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
                if (a.hide_label) continue;
                const t = timeAt(a.bar_index);
                if (t === null) continue;
                const aboveBar = (mw.direction === 1 && i % 2 === 1) || (mw.direction === -1 && i % 2 === 0);
                const text = a.label_override ?? labels[i - 1];
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
        if (tStart !== null && tEnd !== null) {
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
        // the candle pane). △XAB and △BCD each get a filled triangle;
        // together they form the textbook bow-tie shape (Scott Carney
        // reference rendering). Lightweight-charts' primitive API
        // handles the per-frame time→pixel mapping so polygons stay
        // locked to their anchor bars through zoom/pan.
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
        addTriangle(0, 1, 2); // △XAB
        addTriangle(2, 3, 4); // △BCD

        // XABCD polyline.
        const poly = chart.addSeries(LineSeries, {
          color: stroke,
          lineWidth: 2,
          lineStyle: pat.invalidated ? LineStyle.Dashed : LineStyle.Solid,
          priceLineVisible: false,
          lastValueVisible: false,
        });
        poly.setData(clean);
        overlaySeriesRef.current.push(poly);

        // Labels X/A/B/C/D — high pivots above, low pivots below.
        // Pattern alternates: even indices (X, B, D) share X's kind,
        // odd (A, C) are the opposite. For a bullish XABCD the start X
        // is a low, so evens are lows and labels go below; odds are
        // highs and go above. Mirror for bearish.
        for (let i = 0; i < pat.anchors.length; i++) {
          const a = pat.anchors[i];
          const t = timeAt(a.bar_index);
          if (t === null) continue;
          const evenShareIsLow = bull; // bull → X low → even=low
          const isLow = (i % 2 === 0) === evenShareIsLow;
          attachLabel(t, a.price, a.label, labelColor, isLow ? "below" : "above");
        }

        // PRZ — Potential Reversal Zone at D. Height ≈ 2% of XA (fib
        // cluster tolerance Carney uses in the "Harmonic Trading" books).
        // Anchored from D's bar to end of chart so the reader can see
        // whether price actually reverses off it.
        const xPrice = pat.anchors[0].price;
        const aPrice = pat.anchors[1].price;
        const dPrice = pat.anchors[4].price;
        const xa = Math.abs(aPrice - xPrice);
        const przHalf = xa * 0.02;
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

        // Pattern name label — above X anchor for bullish, below for bearish.
        const tX = timeAt(pat.anchors[0].bar_index);
        if (tX !== null) {
          const niceName = pat.subkind
            .replace(/_/g, " ")
            .replace(/\b\w/g, (c) => c.toUpperCase());
          attachLabel(tX, pat.anchors[0].price, niceName, labelColor, bull ? "above" : "below");
        }
      }
    }
  }, [
    data.data, pineOutput, slots,
    showFibBand, showHhLl, onlyLatestMotive, showZigzag, showElliott, fibExtend,
    showImpulse, showZigzagAbc,
    showFlatRegular, showFlatExpanded, showFlatRunning,
    showTriContracting, showTriExpanding, showTriBarrier,
    showHarmonic, harmonicOutput,
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
    <div className="flex h-full flex-col gap-3 p-3">
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
        </div>
        {/* Per-formation toggles — uncheck to prove the backend really
            detected each pattern family rather than drawing defaults. */}
        <div className="flex w-full flex-wrap items-center gap-2 rounded border border-zinc-800 bg-zinc-950/40 p-2 text-xs">
          <span className="font-mono text-[10px] uppercase tracking-wider text-zinc-500">formations</span>
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
          <span className="text-zinc-700">·</span>
          <label className="flex cursor-pointer items-center gap-1">
            <input type="checkbox" checked={showHarmonic} onChange={(e) => setShowHarmonic(e.target.checked)} />
            Harmonic (XABCD)
          </label>
          {showHarmonic && harmonicOutput && (
            <span className="ml-1 font-mono text-[11px] text-zinc-400">
              {harmonicOutput.patterns.length} pattern(s)
            </span>
          )}
        </div>
        <div className="ml-auto text-xs text-zinc-500">
          {data.isFetching ? "Fetching… " : ""}
          {data.data && `${data.data.candles.length} candles · ${totalMotives} motive`}
        </div>
      </div>

      <div ref={containerRef} className="flex-1 rounded border border-zinc-800" />

      <div className="text-xs text-zinc-500">
        Zigzag: <code className="rounded bg-zinc-900 px-1">GET /v2/zigzag/...</code> ·
        Elliott: <code className="rounded bg-zinc-900 px-1">GET /v2/elliott/...</code>
      </div>
    </div>
  );
}
