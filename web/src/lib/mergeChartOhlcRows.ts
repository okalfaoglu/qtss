import type { ChartOhlcRow } from "./marketBarsToCandles";
import { chartOhlcRowsSortedChrono } from "./chartRowsToOhlcBars";

/** Aynı `open_time` anahtarında güncelleme; ardından zamana göre artan sıra. */
export function mergeChartOhlcRowsByOpenTime(existing: ChartOhlcRow[], delta: ChartOhlcRow[]): ChartOhlcRow[] {
  if (!delta.length) return chartOhlcRowsSortedChrono(existing);
  const map = new Map<string, ChartOhlcRow>();
  for (const r of chartOhlcRowsSortedChrono(existing)) {
    map.set(r.open_time, r);
  }
  for (const r of delta) {
    map.set(r.open_time, r);
  }
  return chartOhlcRowsSortedChrono([...map.values()]);
}
