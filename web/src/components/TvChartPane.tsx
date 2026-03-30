import { useCallback, useEffect, useRef } from "react";
import { ColorType, createChart, CrosshairMode, LineStyle } from "lightweight-charts";
import type {
  CandlestickData,
  IChartApi,
  ISeriesApi,
  SeriesMarker,
  UTCTimestamp,
} from "lightweight-charts";

const FIBO_LEVELS: { ratio: number; label: string }[] = [
  { ratio: 0, label: "0%" },
  { ratio: 0.236, label: "23.6%" },
  { ratio: 0.382, label: "38.2%" },
  { ratio: 0.5, label: "50%" },
  { ratio: 0.618, label: "61.8%" },
  { ratio: 0.786, label: "78.6%" },
  { ratio: 1, label: "100%" },
];

function formatChartPrice(n: number): string {
  const a = Math.abs(n);
  if (a >= 10_000) return n.toFixed(2);
  if (a >= 1) return n.toFixed(4);
  return n.toFixed(6);
}

function barsBetweenInclusive(candles: CandlestickData<UTCTimestamp>[], tLo: number, tHi: number): number {
  let n = 0;
  for (const c of candles) {
    const ct = c.time as number;
    if (ct >= tLo && ct <= tHi) n += 1;
  }
  return n;
}
import {
  chartResetView,
  chartScrollLeft,
  chartScrollRight,
  chartZoomIn,
  chartZoomOut,
} from "../lib/chartTimeScaleNav";
import { marketBarsToCandles, type ChartOhlcRow } from "../lib/marketBarsToCandles";
import type { PatternLayerOverlay, ZigzagLayerKind } from "../lib/patternDrawingBatchOverlay";
import type { ChartTool } from "./ChartToolbar";

type LinePoint = { time: UTCTimestamp; value: number };

type Theme = "dark" | "light";

/** API `max_matches` üst sınırı (32) ile hizalı — TV’deki gibi çoklu formasyon katmanı. */
const MAX_PATTERN_LAYERS = 32;

const UPPER_PALETTE_DARK = ["#5c8bd6", "#7e57c2", "#26a69a", "#ff9800", "#ec407a", "#42a5f5"];
const LOWER_PALETTE_DARK = ["#d4a574", "#ba68c8", "#66bb6a", "#ffb74d", "#f48fb1", "#66bb6a"];
const ZZ_PALETTE_DARK = ["#5b8cff", "#b388ff", "#69f0ae", "#ffd54f", "#f06292", "#4fc3f7"];

const UPPER_PALETTE_LIGHT = ["#1565c0", "#6a1b9a", "#2e7d32", "#ef6c00", "#c2185b", "#0277bd"];
const LOWER_PALETTE_LIGHT = ["#e65100", "#8e24aa", "#388e3c", "#f57c00", "#ad1457", "#00838f"];
const ZZ_PALETTE_LIGHT = ["#1565c0", "#7b1fa2", "#388e3c", "#f9a825", "#c62828", "#00695c"];

/** LWC `setData` zamanı katı artan ister (`prev < current`); aynı saniye veya ters sıra assertion. */
function lineSeriesDataStrictAsc(points: LinePoint[]): LinePoint[] {
  const sorted = [...points].sort((a, b) => (a.time as number) - (b.time as number));
  const out: LinePoint[] = [];
  for (const p of sorted) {
    const t = p.time as number;
    if (!Number.isFinite(t)) continue;
    const last = out[out.length - 1];
    if (!last) {
      out.push(p);
      continue;
    }
    const lt = last.time as number;
    if (t > lt) out.push(p);
    else if (t === lt) out[out.length - 1] = p;
  }
  return out;
}

/** Mum verisi olmayan gelecek zamanlarda projeksiyon çizgisini göstermek için sağ ucu genişlet. */
function extendTimeScaleForElliottProjection(
  chart: IChartApi,
  candles: CandlestickData<UTCTimestamp>[],
  layers: PatternLayerOverlay[] | null | undefined,
): void {
  if (!candles.length || !layers?.length) return;
  const lastT = candles[candles.length - 1].time as number;
  let maxProj = lastT;
  for (const L of layers) {
    if (L.zigzagKind !== "elliott_projection") continue;
    for (const p of L.zigzag ?? []) {
      const t = p.time as number;
      if (Number.isFinite(t) && t > maxProj) maxProj = t;
    }
  }
  if (maxProj <= lastT) return;
  const pad = Math.max(Math.floor((maxProj - lastT) * 0.08), 86_400);
  const toT = (maxProj + pad) as UTCTimestamp;
  let vr = chart.timeScale().getVisibleRange();
  if (!vr) {
    chart.timeScale().fitContent();
    vr = chart.timeScale().getVisibleRange();
  }
  if (!vr) return;
  if ((vr.to as number) < toT) {
    chart.timeScale().setVisibleRange({ from: vr.from, to: toT });
  }
}

function zigzagLineOptions(
  kind: ZigzagLayerKind | undefined,
  chartTheme: Theme,
  layerIndex: number,
  lineColorOverride?: string,
  lineStyleOverride?: "solid" | "dotted" | "dashed",
  lineWidthOverride?: number,
): { color: string; lineWidth: number; lineStyle: LineStyle } {
  const zzPal = chartTheme === "dark" ? ZZ_PALETTE_DARK : ZZ_PALETTE_LIGHT;
  const mapLineStyle = (s?: "solid" | "dotted" | "dashed"): LineStyle | undefined =>
    s === "solid" ? LineStyle.Solid : s === "dotted" ? LineStyle.Dotted : s === "dashed" ? LineStyle.Dashed : undefined;
  const applyOverride = (base: { color: string; lineWidth: number; lineStyle: LineStyle }) => {
    const o = lineColorOverride?.trim();
    const lw = typeof lineWidthOverride === "number" && Number.isFinite(lineWidthOverride)
      ? Math.min(6, Math.max(1, Math.round(lineWidthOverride)))
      : base.lineWidth;
    const ls = mapLineStyle(lineStyleOverride) ?? base.lineStyle;
    if (o && /^#[0-9A-Fa-f]{3}$/.test(o)) return { ...base, color: o, lineWidth: lw, lineStyle: ls };
    if (o && /^#[0-9A-Fa-f]{6}$/.test(o)) return { ...base, color: o, lineWidth: lw, lineStyle: ls };
    return { ...base, lineWidth: lw, lineStyle: ls };
  };
  switch (kind) {
    case "elliott_abc":
      return applyOverride({ color: "#ffb74d", lineWidth: 2, lineStyle: LineStyle.Dotted });
    /** Dalga 2/4 içi a–b–c (ana itkı segmenti içinde). */
    case "elliott_abc_sub":
      return applyOverride({ color: "#ffcc80", lineWidth: 2, lineStyle: LineStyle.Dashed });
    case "elliott_v2_macro":
      return applyOverride({ color: "#1E88E5", lineWidth: 4, lineStyle: LineStyle.Solid });
    case "elliott_v2_intermediate":
      return applyOverride({ color: "#43A047", lineWidth: 3, lineStyle: LineStyle.Dashed });
    case "elliott_v2_micro":
      return applyOverride({ color: "#FB8C00", lineWidth: 2, lineStyle: LineStyle.Dotted });
    case "elliott_v2_zigzag_macro":
      return applyOverride({ color: "#90CAF9", lineWidth: 2, lineStyle: LineStyle.Dashed });
    case "elliott_v2_zigzag_intermediate":
      return applyOverride({ color: "#A5D6A7", lineWidth: 2, lineStyle: LineStyle.Dashed });
    case "elliott_v2_zigzag_micro":
      return applyOverride({ color: "#FFCC80", lineWidth: 1, lineStyle: LineStyle.Dashed });
    case "elliott_v2_hist_macro":
      return applyOverride({ color: "#64B5F6", lineWidth: 2, lineStyle: LineStyle.Dotted });
    case "elliott_v2_hist_intermediate":
      return applyOverride({ color: "#66BB6A", lineWidth: 2, lineStyle: LineStyle.Dotted });
    case "elliott_v2_hist_micro":
      return applyOverride({ color: "#FFB74D", lineWidth: 1, lineStyle: LineStyle.Dotted });
    case "elliott_projection":
      return applyOverride({ color: "#2196F3", lineWidth: 2, lineStyle: LineStyle.Dashed });
    case "elliott_projection_done":
      return applyOverride({ color: "#42A5F5", lineWidth: 3, lineStyle: LineStyle.Solid });
    case "elliott_projection_c_active":
      return applyOverride({ color: "#FFB74D", lineWidth: 2, lineStyle: LineStyle.Dashed });
    case "trading_range_mid":
      return applyOverride({ color: "#ffb300", lineWidth: 2, lineStyle: LineStyle.Dotted });
    case "range_position_long":
      return applyOverride({ color: "#00c853", lineWidth: 2, lineStyle: LineStyle.Dashed });
    case "range_position_short":
      return applyOverride({ color: "#ff1744", lineWidth: 2, lineStyle: LineStyle.Dashed });
    default:
      return applyOverride({
        color: zzPal[layerIndex % zzPal.length],
        lineWidth: layerIndex === 0 ? 2 : 1,
        lineStyle: LineStyle.Dashed,
      });
  }
}

type Props = {
  bars: ChartOhlcRow[] | null;
  theme: Theme;
  /** Tam veri yüklemesi (sembol/interval) sonrası artar; yalnızca bu değişince `fitContent` (canlı poll zoom’u bozmaz). */
  fitSessionKey: number;
  activeTool: ChartTool;
  clearDrawNonce: number;
  pivotMarkers?: SeriesMarker<UTCTimestamp>[] | null;
  /** Çoklu formasyon: aynı indekste üst / alt / zigzag hizalı (`zigzagKind` = Elliott çizim stili). */
  patternLayers?: PatternLayerOverlay[] | null;
  pivotLabelMarkers?: SeriesMarker<UTCTimestamp>[] | null;
  patternLabelMarkers?: SeriesMarker<UTCTimestamp>[] | null;
};

type UserDrawing =
  | { kind: "hline"; price: number }
  | { kind: "vline"; time: number }
  | { kind: "trend" | "ray"; t1: number; p1: number; t2: number; p2: number }
  | { kind: "rect"; t1: number; p1: number; t2: number; p2: number }
  | { kind: "fibo"; t1: number; p1: number; t2: number; p2: number }
  | { kind: "measure"; t1: number; p1: number; t2: number; p2: number };

function chartLayout(theme: Theme) {
  if (theme === "dark") {
    return {
      background: { type: ColorType.Solid, color: "#131722" },
      textColor: "#b2b5be",
      grid: {
        vertLines: { color: "#2a2e39" },
        horzLines: { color: "#2a2e39" },
      },
    };
  }
  return {
    background: { type: ColorType.Solid, color: "#ffffff" },
    textColor: "#131722",
    grid: {
      vertLines: { color: "#e0e3eb" },
      horzLines: { color: "#e0e3eb" },
    },
  };
}

function candleStyle(theme: Theme) {
  if (theme === "dark") {
    return {
      upColor: "#26a69a",
      downColor: "#ef5350",
      borderUpColor: "#26a69a",
      borderDownColor: "#ef5350",
      wickUpColor: "#26a69a",
      wickDownColor: "#ef5350",
    };
  }
  return {
    upColor: "#089981",
    downColor: "#f23645",
    borderUpColor: "#089981",
    borderDownColor: "#f23645",
    wickUpColor: "#089981",
    wickDownColor: "#f23645",
  };
}

export function TvChartPane({
  bars,
  theme,
  fitSessionKey,
  activeTool,
  clearDrawNonce,
  pivotMarkers,
  patternLayers,
  pivotLabelMarkers,
  patternLabelMarkers,
}: Props) {
  const mergeAndSortMarkers = (
    base: SeriesMarker<UTCTimestamp>[] | null | undefined,
    pivotLbl: SeriesMarker<UTCTimestamp>[] | null | undefined,
    patternLbl: SeriesMarker<UTCTimestamp>[] | null | undefined,
  ): SeriesMarker<UTCTimestamp>[] =>
    [...(base ?? []), ...(pivotLbl ?? []), ...(patternLbl ?? [])].sort(
      (a, b) => (a.time as number) - (b.time as number),
    );

  const wrapRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<"Candlestick"> | null>(null);
  const upperRefs = useRef<ISeriesApi<"Line">[]>([]);
  const lowerRefs = useRef<ISeriesApi<"Line">[]>([]);
  const zigRefs = useRef<ISeriesApi<"Line">[]>([]);
  const drawSeriesRefs = useRef<ISeriesApi<"Line">[]>([]);
  const userDrawingsRef = useRef<UserDrawing[]>([]);
  const pendingPointRef = useRef<{ time: number; price: number } | null>(null);
  const barsRef = useRef(bars);
  barsRef.current = bars;
  const markersRef = useRef(pivotMarkers ?? []);
  markersRef.current = pivotMarkers ?? [];
  const layersRef = useRef(patternLayers ?? null);
  layersRef.current = patternLayers ?? null;
  const pivotLabelMarkersRef = useRef(pivotLabelMarkers ?? []);
  pivotLabelMarkersRef.current = pivotLabelMarkers ?? [];
  const patternLabelMarkersRef = useRef(patternLabelMarkers ?? []);
  patternLabelMarkersRef.current = patternLabelMarkers ?? [];
  const lastFitSessionKeyRef = useRef<number | undefined>(undefined);
  const activeToolRef = useRef<ChartTool>(activeTool);
  activeToolRef.current = activeTool;

  const applyPatternLayers = (
    uppers: ISeriesApi<"Line">[],
    lowers: ISeriesApi<"Line">[],
    zigs: ISeriesApi<"Line">[],
    layers: PatternLayerOverlay[] | null | undefined,
    chartTheme: Theme,
  ) => {
    const L = layers ?? [];
    for (let i = 0; i < MAX_PATTERN_LAYERS; i++) {
      const layer = L[i];
      const upperLine = uppers[i];
      const lowerLine = lowers[i];
      const zigLine = zigs[i];
      if (upperLine) upperLine.setData(lineSeriesDataStrictAsc(layer?.upper ?? []));
      if (lowerLine) lowerLine.setData(lineSeriesDataStrictAsc(layer?.lower ?? []));
      if (zigLine) {
        const kind =
          layer?.zigzag && layer.zigzag.length > 0 ? layer.zigzagKind ?? "default" : "default";
        zigLine.applyOptions({
          ...zigzagLineOptions(kind, chartTheme, i, layer?.zigzagLineColor, layer?.zigzagLineStyle, layer?.zigzagLineWidth),
          priceLineVisible: false,
          lastValueVisible: false,
        });
        zigLine.setData(lineSeriesDataStrictAsc(layer?.zigzag ?? []));
        const zm = layer?.zigzagMarkers ?? [];
        zigLine.setMarkers(
          zm.length
            ? [...zm].sort((a, b) => (a.time as number) - (b.time as number))
            : [],
        );
      }
    }
  };

  const clearUserDrawSeries = (chart: IChartApi) => {
    for (const s of drawSeriesRefs.current) chart.removeSeries(s);
    drawSeriesRefs.current = [];
  };

  const applyUserDrawings = (
    chart: IChartApi,
    candles: CandlestickData<UTCTimestamp>[],
    chartTheme: Theme,
  ) => {
    clearUserDrawSeries(chart);
    const draws = userDrawingsRef.current;
    if (!draws.length || !candles.length) return;
    const color = chartTheme === "dark" ? "#ffd54f" : "#5d4037";
    const minP = Math.min(...candles.map((c) => c.low));
    const maxP = Math.max(...candles.map((c) => c.high));
    const firstT = candles[0]!.time as number;
    const lastT = candles[candles.length - 1]!.time as number;
    const span = Math.max(60, lastT - firstT);
    const rayT = lastT + span;

    const addLine = (
      points: LinePoint[],
      lineStyle: LineStyle = LineStyle.Solid,
      lineWidth = 2,
      c = color,
      markers?: SeriesMarker<UTCTimestamp>[],
    ) => {
      const s = chart.addLineSeries({
        color: c,
        lineWidth,
        lineStyle,
        priceLineVisible: false,
        lastValueVisible: false,
      });
      s.setData(lineSeriesDataStrictAsc(points));
      if (markers?.length) {
        s.setMarkers([...markers].sort((a, b) => (a.time as number) - (b.time as number)));
      }
      drawSeriesRefs.current.push(s);
    };

    for (const d of draws) {
      if (d.kind === "hline") {
        addLine(
          [
            { time: firstT as UTCTimestamp, value: d.price },
            { time: lastT as UTCTimestamp, value: d.price },
          ],
          LineStyle.Dashed,
          2,
        );
      } else if (d.kind === "vline") {
        addLine(
          [
            { time: d.time as UTCTimestamp, value: minP },
            { time: d.time as UTCTimestamp, value: maxP },
          ],
          LineStyle.Dashed,
          1,
        );
      } else if (d.kind === "trend") {
        addLine(
          [
            { time: d.t1 as UTCTimestamp, value: d.p1 },
            { time: d.t2 as UTCTimestamp, value: d.p2 },
          ],
          LineStyle.Solid,
          2,
        );
      } else if (d.kind === "ray") {
        const dt = Math.max(1, d.t2 - d.t1);
        const slope = (d.p2 - d.p1) / dt;
        const pEnd = d.p2 + slope * (rayT - d.t2);
        addLine(
          [
            { time: d.t1 as UTCTimestamp, value: d.p1 },
            { time: rayT as UTCTimestamp, value: pEnd },
          ],
          LineStyle.Solid,
          2,
        );
      } else if (d.kind === "rect") {
        const t1 = Math.min(d.t1, d.t2);
        const t2 = Math.max(d.t1, d.t2);
        const p1 = Math.min(d.p1, d.p2);
        const p2 = Math.max(d.p1, d.p2);
        const ls = LineStyle.Dotted;
        addLine([{ time: t1 as UTCTimestamp, value: p2 }, { time: t2 as UTCTimestamp, value: p2 }], ls, 1);
        addLine([{ time: t1 as UTCTimestamp, value: p1 }, { time: t2 as UTCTimestamp, value: p1 }], ls, 1);
        addLine([{ time: t1 as UTCTimestamp, value: p1 }, { time: t1 as UTCTimestamp, value: p2 }], ls, 1);
        addLine([{ time: t2 as UTCTimestamp, value: p1 }, { time: t2 as UTCTimestamp, value: p2 }], ls, 1);
      } else if (d.kind === "fibo") {
        const t1 = Math.min(d.t1, d.t2);
        const t2 = Math.max(d.t1, d.t2);
        const hi = Math.max(d.p1, d.p2);
        const lo = Math.min(d.p1, d.p2);
        const range = Math.max(1e-8, hi - lo);
        const fibColor = chartTheme === "dark" ? "#90caf9" : "#1565c0";
        const labelColor = chartTheme === "dark" ? "#e3f2fd" : "#0d47a1";
        for (const { ratio: lv, label } of FIBO_LEVELS) {
          const y = hi - range * lv;
          addLine(
            [{ time: t1 as UTCTimestamp, value: y }, { time: t2 as UTCTimestamp, value: y }],
            LineStyle.Dashed,
            lv === 0.5 ? 2 : 1,
            fibColor,
            [
              {
                time: t2 as UTCTimestamp,
                position: "belowBar",
                color: labelColor,
                shape: "circle",
                text: `${label} (${formatChartPrice(y)})`,
              },
            ],
          );
        }
      } else if (d.kind === "measure") {
        const t1 = d.t1 as UTCTimestamp;
        const t2 = d.t2 as UTCTimestamp;
        const dp = d.p2 - d.p1;
        const deltaStr = (dp >= 0 ? "+" : "") + formatChartPrice(dp);
        const pct =
          d.p1 !== 0 && Number.isFinite(d.p1) ? `${((dp / d.p1) * 100).toFixed(2)}%` : "—";
        const tLo = Math.min(d.t1, d.t2);
        const tHi = Math.max(d.t1, d.t2);
        const barN = barsBetweenInclusive(candles, tLo, tHi);
        const barLabel = barN === 1 ? "1 mum" : `${barN} mum`;
        const measureText = `${deltaStr} (${pct}) · ${barLabel}`;
        const measureColor = chartTheme === "dark" ? "#ce93d8" : "#6a1b9a";
        addLine(
          [{ time: t1, value: d.p1 }, { time: t2, value: d.p2 }],
          LineStyle.Solid,
          2,
          measureColor,
          [
            {
              time: t2,
              position: dp >= 0 ? "belowBar" : "aboveBar",
              color: measureColor,
              shape: "square",
              text: measureText,
            },
          ],
        );
      }
    }
  };

  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;

    const layout = chartLayout(theme);
    const chart = createChart(el, {
      layout: {
        background: layout.background,
        textColor: layout.textColor,
      },
      grid: layout.grid,
      rightPriceScale: { borderVisible: false },
      timeScale: { borderVisible: false, timeVisible: true, secondsVisible: false },
      crosshair: { mode: CrosshairMode.Normal },
    });

    const series = chart.addCandlestickSeries(candleStyle(theme));
    const upPal = theme === "dark" ? UPPER_PALETTE_DARK : UPPER_PALETTE_LIGHT;
    const loPal = theme === "dark" ? LOWER_PALETTE_DARK : LOWER_PALETTE_LIGHT;
    const zzPal = theme === "dark" ? ZZ_PALETTE_DARK : ZZ_PALETTE_LIGHT;

    const uppers: ISeriesApi<"Line">[] = [];
    const lowers: ISeriesApi<"Line">[] = [];
    const zigs: ISeriesApi<"Line">[] = [];

    for (let i = 0; i < MAX_PATTERN_LAYERS; i++) {
      const lw = i === 0 ? 2 : 1;
      uppers.push(
        chart.addLineSeries({
          color: upPal[i % upPal.length],
          lineWidth: lw,
          priceLineVisible: false,
          lastValueVisible: false,
        }),
      );
      lowers.push(
        chart.addLineSeries({
          color: loPal[i % loPal.length],
          lineWidth: lw,
          priceLineVisible: false,
          lastValueVisible: false,
        }),
      );
      zigs.push(
        chart.addLineSeries({
          color: zzPal[i % zzPal.length],
          lineWidth: 1,
          lineStyle: LineStyle.Dashed,
          priceLineVisible: false,
          lastValueVisible: false,
        }),
      );
    }

    chartRef.current = chart;
    seriesRef.current = series;
    upperRefs.current = uppers;
    lowerRefs.current = lowers;
    zigRefs.current = zigs;

    const syncData = () => {
      const data = marketBarsToCandles(barsRef.current);
      series.setData(data);
      series.setMarkers(
        mergeAndSortMarkers(markersRef.current, pivotLabelMarkersRef.current, patternLabelMarkersRef.current),
      );
      applyPatternLayers(uppers, lowers, zigs, layersRef.current ?? []);
      applyUserDrawings(chart, data, theme);
      if (data.length > 0) chart.timeScale().fitContent();
    };
    syncData();

    const onResize = () => {
      const { width, height } = el.getBoundingClientRect();
      chart.resize(Math.floor(width), Math.floor(height));
    };
    onResize();
    const ro = new ResizeObserver(onResize);
    ro.observe(el);

    const onClick = (param: { time?: UTCTimestamp; point?: { x: number; y: number } }) => {
      if (!param.time || !param.point) return;
      const t = param.time as number;
      const price = series.coordinateToPrice(param.point.y);
      if (price == null || !Number.isFinite(price)) return;
      const tool = activeToolRef.current;
      if (tool === "crosshair" || tool === "calc") return;

      if (tool === "hline") {
        userDrawingsRef.current.push({ kind: "hline", price });
      } else if (tool === "vline") {
        userDrawingsRef.current.push({ kind: "vline", time: t });
      } else if (tool === "trend" || tool === "fibo" || tool === "ray" || tool === "rect" || tool === "measure") {
        const p = pendingPointRef.current;
        if (!p) {
          pendingPointRef.current = { time: t, price };
          return;
        }
        if (tool === "trend") userDrawingsRef.current.push({ kind: "trend", t1: p.time, p1: p.price, t2: t, p2: price });
        if (tool === "ray") userDrawingsRef.current.push({ kind: "ray", t1: p.time, p1: p.price, t2: t, p2: price });
        if (tool === "rect") userDrawingsRef.current.push({ kind: "rect", t1: p.time, p1: p.price, t2: t, p2: price });
        if (tool === "fibo") userDrawingsRef.current.push({ kind: "fibo", t1: p.time, p1: p.price, t2: t, p2: price });
        if (tool === "measure") userDrawingsRef.current.push({ kind: "measure", t1: p.time, p1: p.price, t2: t, p2: price });
        pendingPointRef.current = null;
      }

      applyUserDrawings(chart, marketBarsToCandles(barsRef.current), theme);
    };
    chart.subscribeClick(onClick);

    return () => {
      chart.unsubscribeClick(onClick);
      ro.disconnect();
      clearUserDrawSeries(chart);
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
      upperRefs.current = [];
      lowerRefs.current = [];
      zigRefs.current = [];
    };
  }, [theme]);

  useEffect(() => {
    const chart = chartRef.current;
    const series = seriesRef.current;
    if (!series || !chart) return;
    const data = marketBarsToCandles(bars);
    series.setData(data);
    series.setMarkers(mergeAndSortMarkers(pivotMarkers, pivotLabelMarkers, patternLabelMarkers));
    applyPatternLayers(upperRefs.current, lowerRefs.current, zigRefs.current, patternLayers, theme);
    applyUserDrawings(chart, data, theme);
    if (data.length === 0) return;
    if (fitSessionKey !== lastFitSessionKeyRef.current) {
      lastFitSessionKeyRef.current = fitSessionKey;
      chart.timeScale().fitContent();
    }
    extendTimeScaleForElliottProjection(chart, data, patternLayers ?? null);
  }, [bars, pivotMarkers, patternLayers, pivotLabelMarkers, patternLabelMarkers, fitSessionKey, theme]);

  useEffect(() => {
    const chart = chartRef.current;
    if (!chart) return;
    pendingPointRef.current = null;
  }, [activeTool]);

  useEffect(() => {
    const chart = chartRef.current;
    if (!chart) return;
    userDrawingsRef.current = [];
    pendingPointRef.current = null;
    clearUserDrawSeries(chart);
  }, [clearDrawNonce]);

  const onNavZoomOut = useCallback(() => {
    const c = chartRef.current;
    if (c) chartZoomOut(c);
  }, []);
  const onNavZoomIn = useCallback(() => {
    const c = chartRef.current;
    if (c) chartZoomIn(c);
  }, []);
  const onNavLeft = useCallback(() => {
    const c = chartRef.current;
    if (c) chartScrollLeft(c);
  }, []);
  const onNavRight = useCallback(() => {
    const c = chartRef.current;
    if (c) chartScrollRight(c);
  }, []);
  const onNavReset = useCallback(() => {
    const c = chartRef.current;
    if (c) chartResetView(c);
  }, []);

  return (
    <div className="tv-chart-pane-outer">
      <div className="tv-chart-pane" ref={wrapRef} role="img" aria-label="Mum grafiği" />
      {bars?.length ? (
        <nav className="tv-chart-nav" aria-label="Grafik görünümü">
          <div className="tv-chart-nav__group">
            <button
              type="button"
              className="tv-chart-nav__btn"
              onClick={onNavZoomOut}
              title="Uzaklaştır"
              aria-label="Uzaklaştır"
            >
              −
            </button>
            <button
              type="button"
              className="tv-chart-nav__btn"
              onClick={onNavZoomIn}
              title="Yakınlaştır"
              aria-label="Yakınlaştır"
            >
              +
            </button>
          </div>
          <div className="tv-chart-nav__sep" aria-hidden />
          <div className="tv-chart-nav__group">
            <button
              type="button"
              className="tv-chart-nav__btn"
              onClick={onNavLeft}
              title="Sola kaydır (geçmiş)"
              aria-label="Sola kaydır"
            >
              ‹
            </button>
            <button
              type="button"
              className="tv-chart-nav__btn"
              onClick={onNavRight}
              title="Sağa kaydır (güncel)"
              aria-label="Sağa kaydır"
            >
              ›
            </button>
          </div>
          <div className="tv-chart-nav__sep" aria-hidden />
          <div className="tv-chart-nav__group">
            <button
              type="button"
              className="tv-chart-nav__btn tv-chart-nav__btn--reset"
              onClick={onNavReset}
              title="Görünümü sıfırla (tüm veriyi sığdır)"
              aria-label="Görünümü sıfırla"
            >
              ↻
            </button>
          </div>
        </nav>
      ) : null}
      {!bars?.length ? (
        <div className="tv-chart-pane__empty muted" style={{ pointerEvents: "none" }}>
          Üst çubukta sembol ve zaman dilimi seçildiğinde grafik otomatik yüklenir (girişsiz Binance spot). Giriş
          yaptıysanız veri <code>market_bars</code>’tan gelir; kayıt yoksa çekmede REST doldurun.
        </div>
      ) : null}
    </div>
  );
}
