import type { ChartOhlcRow } from "./marketBarsToCandles";
import { chartOhlcRowsSortedChrono } from "./chartRowsToOhlcBars";

/** Ham ISO farkları (örn. `.000Z` vs `Z`) aynı anı tek anahtar yapar. */
function openTimeMergeKey(open_time: string): string {
  const t = new Date(open_time).getTime();
  return Number.isFinite(t) ? String(t) : open_time;
}

/** Aynı `open_time` anında güncelleme; ardından zamana göre artan sıra. */
export function mergeChartOhlcRowsByOpenTime(existing: ChartOhlcRow[], delta: ChartOhlcRow[]): ChartOhlcRow[] {
  if (!delta.length) return chartOhlcRowsSortedChrono(existing);
  const map = new Map<string, ChartOhlcRow>();
  for (const r of chartOhlcRowsSortedChrono(existing)) {
    map.set(openTimeMergeKey(r.open_time), r);
  }
  for (const r of delta) {
    map.set(openTimeMergeKey(r.open_time), r);
  }
  return chartOhlcRowsSortedChrono([...map.values()]);
}
