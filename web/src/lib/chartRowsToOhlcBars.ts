import type { ChartOhlcRow } from "./marketBarsToCandles";

/** Backend `qtss_chart_patterns::OhlcBar` (bar_index = kronolojik sıra, 0..n-1). */
export type OhlcBarJson = {
  bar_index: number;
  open: number;
  high: number;
  low: number;
  close: number;
  volume?: number;
};

function num(x: string | number): number {
  const v = typeof x === "number" ? x : parseFloat(String(x).replace(",", "."));
  return Number.isFinite(v) ? v : NaN;
}

function sortChronoIndices(rows: ChartOhlcRow[]): Array<{ r: ChartOhlcRow; i: number }> {
  const idx = rows.map((r, i) => ({ r, i }));
  idx.sort((a, b) => {
    const ta = new Date(a.r.open_time).getTime();
    const tb = new Date(b.r.open_time).getTime();
    if (ta !== tb) return ta - tb;
    return a.i - b.i;
  });
  return idx;
}

/** Mumları `open_time` artan (API / tarama ile aynı sıra). */
export function chartOhlcRowsSortedChrono(rows: ChartOhlcRow[]): ChartOhlcRow[] {
  return sortChronoIndices(rows).map(({ r }) => r);
}

/** `open_time` artan sıra; `bar_index` taramada 0..n-1. */
export function chartOhlcRowsToScanBars(rows: ChartOhlcRow[]): OhlcBarJson[] {
  return sortChronoIndices(rows).map(({ r }, j) => {
    const o = num(r.open);
    const h = num(r.high);
    const l = num(r.low);
    const c = num(r.close);
    const v = "volume" in r ? num((r as Record<string, unknown>).volume as string | number) : NaN;
    const bar: OhlcBarJson = { bar_index: j, open: o, high: h, low: l, close: c };
    if (Number.isFinite(v) && v > 0) bar.volume = v;
    return bar;
  });
}
