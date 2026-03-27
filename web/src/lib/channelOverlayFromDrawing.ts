import type { UTCTimestamp } from "lightweight-charts";
import type { ChannelSixDrawingJson } from "../api/client";
import type { ChartOhlcRow } from "./marketBarsToCandles";

export type ChannelOverlayLines = {
  upper: { time: UTCTimestamp; value: number }[];
  lower: { time: UTCTimestamp; value: number }[];
};

function barTimeSec(row: ChartOhlcRow): UTCTimestamp | null {
  const t = Math.floor(new Date(row.open_time).getTime() / 1000);
  return Number.isFinite(t) ? (t as UTCTimestamp) : null;
}

function segment(
  barsChrono: ChartOhlcRow[],
  a: { bar_index: number; price: number },
  b: { bar_index: number; price: number },
): { time: UTCTimestamp; value: number }[] {
  const ia = Math.floor(a.bar_index);
  const ib = Math.floor(b.bar_index);
  if (ia < 0 || ib < 0 || ia >= barsChrono.length || ib >= barsChrono.length) return [];
  const ta = barTimeSec(barsChrono[ia]);
  const tb = barTimeSec(barsChrono[ib]);
  if (ta == null || tb == null) return [];
  const pa = { time: ta, value: a.price };
  const pb = { time: tb, value: b.price };
  return ta <= tb ? [pa, pb] : [pb, pa];
}

/**
 * API `drawing` → LWC çizgi noktaları.
 * `barsChrono`, channel-six isteğindeki `bars` ile aynı dilim olmalı (`bar_index` 0 = ilk mum).
 */
export function channelDrawingToOverlay(
  barsChrono: ChartOhlcRow[],
  drawing: ChannelSixDrawingJson | undefined,
): ChannelOverlayLines | null {
  if (!drawing || !barsChrono.length) return null;
  const upper = segment(barsChrono, drawing.upper[0], drawing.upper[1]);
  const lower = segment(barsChrono, drawing.lower[0], drawing.lower[1]);
  if (upper.length < 2 || lower.length < 2) return null;
  return { upper, lower };
}
