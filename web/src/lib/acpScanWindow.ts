import { chartOhlcRowsSortedChrono } from "./chartRowsToOhlcBars";
import type { ChartOhlcRow } from "./marketBarsToCandles";

/**
 * ACP tarama penceresi: TV `calculated_bars` son N mum.
 * `repaint === false` (Pine varsayılanı): sadece **kapanmış** mumlar — en yeni açık mum düşürülür (`barstate.isconfirmed` benzeri).
 */
export function acpOhlcWindowForScan(
  bars: ChartOhlcRow[] | null | undefined,
  calculatedBars: number,
  repaint: boolean,
): ChartOhlcRow[] {
  if (!bars?.length) return [];
  const chrono = chartOhlcRowsSortedChrono(bars);
  const cap = Math.min(Math.max(1, calculatedBars), chrono.length);
  let w = chrono.slice(-cap);
  if (!repaint && w.length > 1) {
    w = w.slice(0, -1);
  }
  return w;
}
