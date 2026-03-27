import type { ChartOhlcRow } from "./marketBarsToCandles";
import { chartOhlcRowsSortedChrono } from "./chartRowsToOhlcBars";

function num(x: string | number): number {
  const v = typeof x === "number" ? x : parseFloat(String(x).replace(",", "."));
  return Number.isFinite(v) ? v : NaN;
}

/** Son mum(lar)dan anlık fiyat hareketi: mum içi (açılışa göre) ve bir önceki kapanışa göre. */
export type LastCandleLiveStats = {
  open: number;
  high: number;
  low: number;
  close: number;
  /** (close - open) / open * 100 */
  pctFromOpen: number | null;
  /** Önceki mum varsa (close - prevClose) / prevClose * 100 */
  pctFromPrevClose: number | null;
};

export function lastCandleLiveStatsFromRows(rows: ChartOhlcRow[] | null | undefined): LastCandleLiveStats | null {
  if (!rows?.length) return null;
  const chrono = chartOhlcRowsSortedChrono(rows);
  const last = chrono[chrono.length - 1];
  const o = num(last.open);
  const h = num(last.high);
  const l = num(last.low);
  const c = num(last.close);
  if (![o, h, l, c].every(Number.isFinite)) return null;

  const pctFromOpen = o !== 0 ? ((c - o) / o) * 100 : null;

  let pctFromPrevClose: number | null = null;
  if (chrono.length >= 2) {
    const prevC = num(chrono[chrono.length - 2].close);
    if (Number.isFinite(prevC) && prevC !== 0) {
      pctFromPrevClose = ((c - prevC) / prevC) * 100;
    }
  }

  return { open: o, high: h, low: l, close: c, pctFromOpen, pctFromPrevClose };
}
