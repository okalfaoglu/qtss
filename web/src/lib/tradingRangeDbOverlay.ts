import type { SeriesMarker, UTCTimestamp } from "lightweight-charts";
import { chartOhlcRowsSortedChrono } from "./chartRowsToOhlcBars";
import type { ChartOhlcRow } from "./marketBarsToCandles";
import type { PatternLayerOverlay } from "./patternDrawingBatchOverlay";

export type TradingRangeDbPayload = {
  range_high?: number;
  range_low?: number;
  mid?: number;
  valid?: boolean;
  is_range_regime?: boolean;
  long_sweep_signal?: boolean;
  short_sweep_signal?: boolean;
  atr?: number;
  atr_sma?: number;
  chart_window_start_open_time?: string;
  chart_window_end_open_time?: string;
  last_bar_open_time?: string;
};

function rowTime(row: ChartOhlcRow): UTCTimestamp | null {
  const t = Math.floor(new Date(row.open_time).getTime() / 1000);
  return Number.isFinite(t) ? (t as UTCTimestamp) : null;
}

/** `analysis_snapshots.payload` (trading_range) → yatay üç çizgi (üst / alt / orta). */
export function patternLayerFromDbTradingRange(
  bars: ChartOhlcRow[],
  payload: unknown,
): PatternLayerOverlay | null {
  if (!payload || typeof payload !== "object") return null;
  const p = payload as TradingRangeDbPayload;
  const rh = p.range_high;
  const rl = p.range_low;
  const mid = p.mid;
  if (
    typeof rh !== "number" ||
    typeof rl !== "number" ||
    typeof mid !== "number" ||
    !Number.isFinite(rh) ||
    !Number.isFinite(rl) ||
    !Number.isFinite(mid)
  ) {
    return null;
  }

  const ch = chartOhlcRowsSortedChrono(bars);
  if (!ch.length) return null;
  const t0 = rowTime(ch[0]!);
  const t1 = rowTime(ch[ch.length - 1]!);
  if (t0 == null || t1 == null) return null;

  return {
    upper: [
      { time: t0, value: rh },
      { time: t1, value: rh },
    ],
    lower: [
      { time: t0, value: rl },
      { time: t1, value: rl },
    ],
    zigzag: [
      { time: t0, value: mid },
      { time: t1, value: mid },
    ],
    zigzagKind: "trading_range_mid",
    zigzagLineColor: "#ffb300",
    zigzagLineStyle: "dotted",
    zigzagLineWidth: 2,
  };
}

/** Son mumda likidite süpürme işareti (DB `trading_range` payload). */
export function sweepMarkersFromDbTradingRange(
  bars: ChartOhlcRow[],
  payload: unknown,
): SeriesMarker<UTCTimestamp>[] {
  if (!payload || typeof payload !== "object") return [];
  const p = payload as TradingRangeDbPayload;
  const ch = chartOhlcRowsSortedChrono(bars);
  if (!ch.length) return [];
  const last = ch[ch.length - 1]!;
  const t = rowTime(last);
  if (t == null) return [];
  const out: SeriesMarker<UTCTimestamp>[] = [];
  if (p.long_sweep_signal) {
    out.push({
      time: t,
      position: "belowBar",
      shape: "arrowUp",
      color: "#089981",
      text: "L sweep",
    });
  }
  if (p.short_sweep_signal) {
    out.push({
      time: t,
      position: "aboveBar",
      shape: "arrowDown",
      color: "#f23645",
      text: "S sweep",
    });
  }
  return out;
}
