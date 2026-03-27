import type { SeriesMarker, UTCTimestamp } from "lightweight-charts";
import type { ChartOhlcRow } from "./marketBarsToCandles";

/** API `outcome.pivots`: `[bar_index, price, dir]` — tarama diliminde `barsChrono[bar_index]`. */
export function buildChannelScanPivotMarkers(
  barsChrono: ChartOhlcRow[],
  pivots: [number, number, number][],
  theme: "dark" | "light",
): SeriesMarker<UTCTimestamp>[] {
  const up = theme === "dark" ? "#26a69a" : "#089981";
  const down = theme === "dark" ? "#ef5350" : "#f23645";
  const out: SeriesMarker<UTCTimestamp>[] = [];
  for (const [bi, _price, dir] of pivots) {
    if (bi < 0 || bi >= barsChrono.length) continue;
    const t = Math.floor(new Date(barsChrono[bi].open_time).getTime() / 1000);
    if (!Number.isFinite(t)) continue;
    const peak = dir > 0;
    out.push({
      time: t as UTCTimestamp,
      position: peak ? "aboveBar" : "belowBar",
      color: peak ? up : down,
      shape: peak ? "arrowDown" : "arrowUp",
      text: peak ? "H" : "L",
      size: 1,
    });
  }
  out.sort((a, b) => (a.time as number) - (b.time as number));
  return out;
}
