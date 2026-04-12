/**
 * Chart.tsx — TradingView lightweight-charts v5 based chart page.
 *
 * Replaces the old SVG-based chart with a proper canvas-rendered chart
 * that handles price scale, time scale, crosshair, zoom, pan natively.
 *
 * All detection overlays, zigzag, Wyckoff, volume, Entry/TP/SL are
 * preserved and rendered via TV primitives / markers / line series.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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

function familyColor(family: string, subkind?: string): string {
  if (family === "range" && subkind && RANGE_SUBKIND_COLORS[subkind]) {
    return RANGE_SUBKIND_COLORS[subkind];
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
    const aP = Number(anchors[1].price);
    const dP = Number(anchors[4].price);
    const adRange = Math.abs(aP - dP);
    const dir = d.subkind.includes("bull") ? 1 : -1;
    entry = dP;
    tp1 = dP + dir * adRange * 0.382;
    tp2 = dP + dir * adRange * 0.618;
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
  const [showZigzag, setShowZigzag] = useState(false);
  const [showVolume, setShowVolume] = useState(true);
  const [familyModes, setFamilyModes] = useState<Record<string, FamilyMode>>({});
  const [detailLayers, setDetailLayers] = useState<Record<string, Set<string>>>({});
  const [activeTool, setActiveTool] = useState<ToolId>("crosshair");

  const fetchingOlderRef = useRef(false);
  const chartContainerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const candleSeriesRef = useRef<ISeriesApi<"Candlestick"> | null>(null);
  const volumeSeriesRef = useRef<ISeriesApi<"Histogram"> | null>(null);
  const overlayLinesRef = useRef<ISeriesApi<"Line">[]>([]);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const markersRef = useRef<any>(null);
  const resizeObserverRef = useRef<ResizeObserver | null>(null);

  // ─── Derived state ──────────────────────────────────────────────
  const cycleFamily = useCallback((family: string) => {
    setFamilyModes((prev) => {
      const cur = prev[family] ?? "on";
      const next: FamilyMode = cur === "off" ? "on" : cur === "on" ? "detail" : "off";
      if (next === "detail") {
        setDetailLayers((dl) => ({ ...dl, [family]: new Set(["entry_tp_sl"]) }));
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
    refetchInterval: 10_000,
  });

  const wyckoffQuery = useQuery({
    queryKey: ["v2", "wyckoff", "overlay", debounced.symbol, debounced.timeframe],
    queryFn: () =>
      apiFetch<{ overlay: WyckoffOverlayData | null }>(
        `/v2/wyckoff/overlay/${debounced.symbol}/${debounced.timeframe}`,
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

  const visibleDetections = useMemo(
    () => merged ? merged.detections.filter((d) => (familyModes[d.family] ?? "on") !== "off") : [],
    [merged, familyModes],
  );

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
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        chart.applyOptions({ width, height });
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
      ro.disconnect();
      chart.remove();
      chartRef.current = null;
      candleSeriesRef.current = null;
      volumeSeriesRef.current = null;
      overlayLinesRef.current = [];
    };
  }, [debounced.venue, debounced.symbol, debounced.timeframe]);

  // ─── Update data ────────────────────────────────────────────────
  useEffect(() => {
    if (!merged || !candleSeriesRef.current || !volumeSeriesRef.current || !chartRef.current) return;

    const chart = chartRef.current;
    const candleSeries = candleSeriesRef.current;
    const volumeSeries = volumeSeriesRef.current;

    // Convert candles — sort + deduplicate (TV requires strictly ascending time)
    const sorted = [...merged.candles].sort(
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

    candleSeries.setData(candleData);
    volumeSeries.setData(showVolume ? volData : []);

    // ── Remove old overlay lines ──
    for (const line of overlayLinesRef.current) {
      chart.removeSeries(line);
    }
    overlayLinesRef.current = [];

    // ── Detection overlays ──
    // For each detection, draw line series for the formation + projections
    for (const d of visibleDetections) {
      const color = familyColor(d.family, d.subkind);
      const isZone = ZONE_BOX_SUBKINDS.has(d.subkind);
      const isDashed = d.state === "forming";
      const isInvalidated = d.state === "invalidated";
      const isHov = hovered === d.id;

      if (d.anchors.length >= 2 && !isZone) {
        // Main formation polyline as line series
        const formLine = chart.addSeries(LineSeries, {
          color: color,
          lineWidth: isHov ? 3 : 2,
          lineStyle: isDashed ? LineStyle.Dashed : isInvalidated ? LineStyle.Dotted : LineStyle.Solid,
          crosshairMarkerVisible: false,
          lastValueVisible: false,
          priceLineVisible: false,
          pointMarkersVisible: true,
          pointMarkersRadius: isHov ? 4 : 3,
        });

        const lineData = d.anchors.map((a) => ({
          time: isoToUnix(a.time),
          value: Number(a.price),
        }));
        formLine.setData(sortLineData(lineData));
        overlayLinesRef.current.push(formLine);

        // Projected anchors (dashed continuation)
        if (d.projected_anchors && d.projected_anchors.length > 0) {
          const projLine = chart.addSeries(LineSeries, {
            color: color,
            lineWidth: isHov ? 3 : 2,
            lineStyle: LineStyle.Dashed,
            crosshairMarkerVisible: false,
            lastValueVisible: false,
            priceLineVisible: false,
            pointMarkersVisible: true,
            pointMarkersRadius: isHov ? 3 : 2,
          });
          const lastAnchor = d.anchors[d.anchors.length - 1];
          const projData = [
            { time: isoToUnix(lastAnchor.time), value: Number(lastAnchor.price) },
            ...d.projected_anchors.map((a) => ({
              time: isoToUnix(a.time),
              value: Number(a.price),
            })),
          ];
          projLine.setData(sortLineData(projData));
          overlayLinesRef.current.push(projLine);
        }

        // Sub-wave decomposition
        if (d.sub_wave_anchors) {
          for (const seg of d.sub_wave_anchors) {
            if (seg.length < 2) continue;
            const swLine = chart.addSeries(LineSeries, {
              color: "#facc15",
              lineWidth: 1,
              lineStyle: LineStyle.Dashed,
              crosshairMarkerVisible: false,
              lastValueVisible: false,
              priceLineVisible: false,
              pointMarkersVisible: true,
              pointMarkersRadius: 2,
            });
            swLine.setData(sortLineData(seg.map((a) => ({
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
          const startTime = isoToUnix(merged.candles[startIdx].open_time);
          hl.setData(sortLineData([
            { time: startTime, value: price },
            { time: endTime, value: price },
          ]));
          overlayLinesRef.current.push(hl);
        }
      }

      // Entry / TP / SL price lines (detail mode or hover)
      const showDetail = (familyModes[d.family] ?? "on") === "detail" || isHov;
      if (showDetail && d.anchors.length > 0) {
        const { entry, tp1, tp2, sl } = computeTargets(d);
        const lastTime = isoToUnix(d.anchors[d.anchors.length - 1].time);
        // Find a time 20 bars into the future
        const barInterval = merged.candles.length >= 2
          ? (new Date(merged.candles[merged.candles.length - 1].open_time).getTime() -
             new Date(merged.candles[merged.candles.length - 2].open_time).getTime()) / 1000
          : 3600;
        const futureTime = (lastTime as number + barInterval * 20) as Time;

        const drawLevel = (price: number | null, col: string, style: LineStyle) => {
          if (!price || !Number.isFinite(price)) return;
          const lvl = chart.addSeries(LineSeries, {
            color: col,
            lineWidth: 1,
            lineStyle: style,
            crosshairMarkerVisible: false,
            lastValueVisible: false,
            priceLineVisible: false,
          });
          lvl.setData(sortLineData([
            { time: lastTime, value: price },
            { time: futureTime, value: price },
          ]));
          overlayLinesRef.current.push(lvl);
        };

        drawLevel(sl, "#ef4444", LineStyle.Dashed);
        drawLevel(entry, "#d4d4d8", LineStyle.Dotted);
        drawLevel(tp1, "#34d399", LineStyle.Dashed);
        drawLevel(tp2, "#34d39980", LineStyle.Dotted);
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
        zigLine.setData(sortLineData(pts.map((p) => ({ time: p.time, value: p.price }))));
        overlayLinesRef.current.push(zigLine);

        // Swing labels as markers on the candle series
        const swingColors: Record<string, string> = {
          HH: "#22c55e", HL: "#4ade80", LH: "#ef4444", LL: "#f87171",
        };
        const markers: SeriesMarker<Time>[] = pts
          .filter((p) => p.swing)
          .map((p) => ({
            time: p.time,
            position: p.kind === "H" ? "aboveBar" as const : "belowBar" as const,
            color: swingColors[p.swing!] ?? "#facc15",
            shape: "circle" as const,
            text: p.swing!,
          }));
        if (markers.length > 0) {
          if (markersRef.current) markersRef.current.detach();
          markersRef.current = createSeriesMarkers(candleSeries, markers);
        }
      }
    } else {
      if (markersRef.current) { markersRef.current.detach(); markersRef.current = null; }
    }

    // ── Wyckoff overlay ──
    const wyckoffOverlay = wyckoffQuery.data?.overlay ?? null;
    if (wyckoffOverlay && (familyModes["wyckoff"] ?? "on") !== "off") {
      const { range: wRange, creek, ice } = wyckoffOverlay;
      const isAccum = wyckoffOverlay.schematic === "accumulation" || wyckoffOverlay.schematic === "reaccumulation";
      const wColor = isAccum ? "#22c55e60" : "#ef444460";

      // Range top/bottom lines
      if (wRange.top != null) {
        const topLine = chart.addSeries(LineSeries, {
          color: wColor,
          lineWidth: 1,
          lineStyle: LineStyle.LargeDashed,
          crosshairMarkerVisible: false,
          lastValueVisible: false,
          priceLineVisible: false,
        });
        const firstT = isoToUnix(merged.candles[0].open_time);
        const lastT = isoToUnix(merged.candles[merged.candles.length - 1].open_time);
        topLine.setData(sortLineData([
          { time: firstT, value: wRange.top },
          { time: lastT, value: wRange.top },
        ]));
        overlayLinesRef.current.push(topLine);
      }
      if (wRange.bottom != null) {
        const botLine = chart.addSeries(LineSeries, {
          color: wColor,
          lineWidth: 1,
          lineStyle: LineStyle.LargeDashed,
          crosshairMarkerVisible: false,
          lastValueVisible: false,
          priceLineVisible: false,
        });
        const firstT = isoToUnix(merged.candles[0].open_time);
        const lastT = isoToUnix(merged.candles[merged.candles.length - 1].open_time);
        botLine.setData(sortLineData([
          { time: firstT, value: wRange.bottom },
          { time: lastT, value: wRange.bottom },
        ]));
        overlayLinesRef.current.push(botLine);
      }
      // Creek line
      if (creek != null) {
        const creekLine = chart.addSeries(LineSeries, {
          color: "#3b82f680",
          lineWidth: 1,
          lineStyle: LineStyle.Dashed,
          crosshairMarkerVisible: false,
          lastValueVisible: false,
          priceLineVisible: false,
        });
        const firstT = isoToUnix(merged.candles[0].open_time);
        const lastT = isoToUnix(merged.candles[merged.candles.length - 1].open_time);
        creekLine.setData(sortLineData([
          { time: firstT, value: creek },
          { time: lastT, value: creek },
        ]));
        overlayLinesRef.current.push(creekLine);
      }
      // Ice line
      if (ice != null) {
        const iceLine = chart.addSeries(LineSeries, {
          color: "#ef444480",
          lineWidth: 1,
          lineStyle: LineStyle.Dashed,
          crosshairMarkerVisible: false,
          lastValueVisible: false,
          priceLineVisible: false,
        });
        const firstT = isoToUnix(merged.candles[0].open_time);
        const lastT = isoToUnix(merged.candles[merged.candles.length - 1].open_time);
        iceLine.setData(sortLineData([
          { time: firstT, value: ice },
          { time: lastT, value: ice },
        ]));
        overlayLinesRef.current.push(iceLine);
      }
    }

  }, [merged, showVolume, showZigzag, visibleDetections, hovered, familyModes, wyckoffQuery.data]);

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
                    className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] uppercase tracking-wide transition"
                    style={{
                      borderWidth: 1,
                      borderColor: isOff ? "#3f3f46" : color,
                      background: isOff ? "transparent" : isDetail ? `${color}33` : `${color}18`,
                      color: isOff ? "#71717a" : color,
                    }}
                    title={isOff ? "Gizli" : isDetail ? "Detay" : "Görünür"}
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
          style={{ minHeight: 400 }}
        />

        {/* ── Detections Table ─────────────────────────────────── */}
        {merged && merged.detections.length > 0 && (
          <div className="max-h-48 overflow-auto border-t border-zinc-800 bg-zinc-950">
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
                  <tr
                    key={d.id}
                    className={`border-t border-zinc-800/40 transition-colors ${
                      hovered === d.id ? "bg-zinc-800/60" : "hover:bg-zinc-900"
                    }`}
                    onMouseEnter={() => setHovered(d.id)}
                    onMouseLeave={() => setHovered(null)}
                  >
                    <td className="px-2 py-0.5" style={{ color: familyColor(d.family, d.subkind) }}>
                      {d.subkind}
                    </td>
                    <td className="px-2 py-0.5 text-zinc-400">{d.state}</td>
                    <td className="px-2 py-0.5 text-zinc-500">{d.anchor_time}</td>
                    <td className="px-2 py-0.5 text-right">{d.anchor_price}</td>
                    <td className="px-2 py-0.5 text-right text-zinc-500">{d.invalidation_price}</td>
                    <td className="px-2 py-0.5 text-right">{d.confidence}</td>
                  </tr>
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
