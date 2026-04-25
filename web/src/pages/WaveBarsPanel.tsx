// FAZ 25.1 — wave-bar candle visualization (the "noise-cleaned candle
// structure" requested in the design conversation). Each candle on
// this chart represents ONE Elliott wave (pivot → next pivot) instead
// of a fixed-time bar:
//   * Open  = price at the starting pivot
//   * Close = price at the ending pivot
//   * High / Low = max / min of the underlying OHLC bars between the
//     two pivots (so the wick still shows max excursion within the wave)
// The X-axis is wave-index (synthetic seconds, 60s apart) so all waves
// have equal visual width — much easier on the eye for Elliott counting
// than time-proportional rendering.
//
// Data source: GET /v2/wave-bars/{exchange}/{symbol}/{tf}.

import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  CandlestickSeries,
  ColorType,
  CrosshairMode,
  createChart,
  type IChartApi,
  type ISeriesApi,
  type Time,
  type UTCTimestamp,
} from "lightweight-charts";

import { apiFetch } from "../lib/api";

type WaveBar = {
  index: number;
  slot: number;
  start_time: string;
  end_time: string;
  start_bar_index: number;
  end_bar_index: number;
  open: number;
  close: number;
  high: number;
  low: number;
  direction: number;       // +1 up, -1 down
  duration_seconds: number;
  bar_count: number;
  size_norm: number;       // dimensionless vs median wave size
  volume_total: number;
};
type WaveBarsResponse = {
  exchange: string;
  segment: string;
  symbol: string;
  timeframe: string;
  slot: number;
  length: number;
  waves: WaveBar[];
};

const SLOT_LABELS = ["Z1 (3)", "Z2 (5)", "Z3 (8)", "Z4 (13)", "Z5 (21)"];

export interface WaveBarsPanelProps {
  exchange?: string;
  symbol?: string;
  segment?: string;
  tf?: string;
}

export function WaveBarsPanel({
  exchange = "binance",
  symbol = "BTCUSDT",
  segment = "futures",
  tf = "4h",
}: WaveBarsPanelProps) {
  const [slot, setSlot] = useState(2); // Z3 default — same as the
                                       // existing chart toolbar default
  const [collapsed, setCollapsed] = useState(false);

  // Limit aligned with the OHLC chart above (LuxAlgoChart pulls 1001
  // candles by default) so the wave bars cover the SAME time window
  // and the two halves are directly comparable.
  const { data, isLoading, isError } = useQuery<WaveBarsResponse>({
    queryKey: ["wave-bars", exchange, symbol, segment, tf, slot],
    queryFn: () =>
      apiFetch(
        `/v2/wave-bars/${exchange}/${symbol}/${tf}?segment=${segment}&slot=${slot}&limit=1000`
      ),
    refetchInterval: 30_000,
  });
  const waves = data?.waves ?? [];

  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<"Candlestick"> | null>(null);

  // Initialise the chart once.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const chart = createChart(el, {
      layout: {
        background: { type: ColorType.Solid, color: "#09090b" },
        textColor: "#a1a1aa",
        fontFamily: "ui-monospace, SFMono-Regular, monospace",
        fontSize: 11,
      },
      grid: {
        vertLines: { color: "#1f2937" },
        horzLines: { color: "#1f2937" },
      },
      crosshair: { mode: CrosshairMode.Normal },
      rightPriceScale: { borderColor: "#27272a" },
      timeScale: {
        borderColor: "#27272a",
        timeVisible: false,
        secondsVisible: false,
        // X-axis is wave-index (synthetic time). Hide the synthetic
        // dates that lightweight-charts would otherwise print and
        // replace each tick label with the underlying wave index.
        tickMarkFormatter: (t: number) => {
          const idx = Math.round((t - 1_700_000_000) / 60);
          return idx >= 0 ? `#${idx}` : "";
        },
      },
      autoSize: true,
    });
    const series = chart.addSeries(CandlestickSeries, {
      upColor: "#22c55e",
      downColor: "#ef4444",
      borderUpColor: "#16a34a",
      borderDownColor: "#dc2626",
      wickUpColor: "#86efac",
      wickDownColor: "#fca5a5",
    });
    chartRef.current = chart;
    seriesRef.current = series;
    return () => {
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
    };
  }, []);

  // Push wave bars into the candlestick series whenever data changes.
  // Synthetic time axis: each wave gets a 60-second slot so visual
  // widths are uniform and lightweight-charts is happy with monotonic
  // increasing timestamps.
  const candlesticks = useMemo(() => {
    return waves.map((w, i) => ({
      time: ((1_700_000_000 + i * 60) as unknown) as UTCTimestamp,
      open: w.open,
      high: w.high,
      low: w.low,
      close: w.close,
    }));
  }, [waves]);

  useEffect(() => {
    const series = seriesRef.current;
    const chart = chartRef.current;
    if (!series || !chart) return;
    series.setData(candlesticks);
    if (candlesticks.length > 0) {
      chart.timeScale().fitContent();
    }
  }, [candlesticks]);

  const stats = useMemo(() => {
    if (waves.length === 0) return null;
    const ups = waves.filter((w) => w.direction === 1).length;
    const downs = waves.length - ups;
    const sizes = waves.map((w) => w.size_norm);
    const maxSize = sizes.reduce((a, b) => Math.max(a, b), 0);
    const totalDuration = waves.reduce((a, w) => a + w.duration_seconds, 0);
    const firstStart = waves[0]?.start_time;
    const lastEnd = waves[waves.length - 1]?.end_time;
    const fmt = (iso?: string) => {
      if (!iso) return "—";
      try {
        return new Date(iso).toLocaleString("tr-TR", {
          year: "numeric",
          month: "short",
          day: "numeric",
        });
      } catch {
        return iso;
      }
    };
    return {
      count: waves.length,
      ups,
      downs,
      maxSize,
      totalHours: totalDuration / 3600,
      rangeStart: fmt(firstStart),
      rangeEnd: fmt(lastEnd),
    };
  }, [waves]);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-zinc-800 bg-zinc-900/40 px-3 py-1.5">
        <div className="flex items-center gap-3">
          <button
            onClick={() => setCollapsed((v) => !v)}
            title={collapsed ? "Genişlet" : "Daralt — chart için yer aç"}
            className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400 hover:bg-zinc-700"
          >
            {collapsed ? "▲ aç" : "▼ kapat"}
          </button>
          <h2 className="text-xs font-semibold text-emerald-300">Wave Bars</h2>
          <span className="text-[10px] text-zinc-500">
            Her mum = bir pivot→pivot dalga. X ekseni dalga-indeksi, zaman değil.
          </span>
        </div>
        <div className="flex items-center gap-1">
          {SLOT_LABELS.map((label, i) => (
            <button
              key={i}
              onClick={() => setSlot(i)}
              className={`rounded px-2 py-0.5 text-[10px] ${
                slot === i
                  ? "bg-emerald-600 text-white"
                  : "bg-zinc-800 text-zinc-400 hover:bg-zinc-700"
              }`}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      <div
        ref={containerRef}
        className="flex-none"
        style={{
          display: collapsed ? "none" : undefined,
          height: collapsed ? 0 : 160,
        }}
      />

      {!collapsed && (
        <div className="flex flex-wrap items-center gap-3 border-t border-zinc-800 bg-zinc-900/40 px-3 py-1 text-[10px] text-zinc-400">
          {isLoading && <span>loading…</span>}
          {isError && <span className="text-red-400">/v2/wave-bars failed</span>}
          {stats && (
            <>
              <span>
                <span className="text-zinc-500">dalgalar</span>{" "}
                <span className="text-zinc-200">{stats.count}</span>
              </span>
              <span>
                <span className="text-emerald-400">▲ {stats.ups}</span>
                {" / "}
                <span className="text-rose-400">▼ {stats.downs}</span>
              </span>
              <span>
                <span className="text-zinc-500">max boy (medyana oran)</span>{" "}
                <span className="font-mono text-zinc-200">
                  {stats.maxSize.toFixed(2)}×
                </span>
              </span>
              <span>
                <span className="text-zinc-500">aralık</span>{" "}
                <span className="font-mono text-zinc-200">
                  {stats.rangeStart} → {stats.rangeEnd}
                </span>
                {" · "}
                <span className="font-mono text-zinc-300">
                  {stats.totalHours.toFixed(0)}h
                </span>
              </span>
              <span className="ml-auto text-zinc-500">
                slot {data?.slot} · length {data?.length}
              </span>
            </>
          )}
        </div>
      )}
    </div>
  );
}

// Avoid unused-import warning when the host page doesn't pass the
// `Time` type explicitly; the type alias keeps the import live for
// clarity at the top of the file.
export type _Time = Time;
