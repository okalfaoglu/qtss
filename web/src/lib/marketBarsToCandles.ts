import type { CandlestickData, UTCTimestamp } from "lightweight-charts";
import type { MarketBarRow } from "../api/client";

/** Grafik için minimum OHLC (API `MarketBarRow` ile uyumlu). */
export type ChartOhlcRow = Pick<MarketBarRow, "open_time" | "open" | "high" | "low" | "close" | "volume">;

/** API `open_time` ISO → saniye; mumlar zamana göre artan, aynı saniyede tekil. */
export function marketBarsToCandles(rows: ChartOhlcRow[] | null | undefined): CandlestickData<UTCTimestamp>[] {
  if (!rows?.length) return [];
  const byTime = new Map<number, CandlestickData<UTCTimestamp>>();
  for (const r of rows) {
    const t = Math.floor(new Date(r.open_time).getTime() / 1000);
    if (!Number.isFinite(t)) continue;
    const o = parseFloat(String(r.open));
    const h = parseFloat(String(r.high));
    const l = parseFloat(String(r.low));
    const c = parseFloat(String(r.close));
    if (![o, h, l, c].every(Number.isFinite)) continue;
    byTime.set(t, { time: t as UTCTimestamp, open: o, high: h, low: l, close: c });
  }
  return Array.from(byTime.entries())
    .sort(([a], [b]) => a - b)
    .map(([, v]) => v);
}
