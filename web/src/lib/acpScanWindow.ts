import { chartOhlcRowsSortedChrono } from "./chartRowsToOhlcBars";
import type { ChartOhlcRow } from "./marketBarsToCandles";

/**
 * ACP (Trendoscope) kanal taraması ve tarama sonucunun grafik üstü hizası.
 *
 * **Elliott (`elliottEngineV2`) bu fonksiyonu kullanmaz.** `App` içindeki `bars` durumu canlı poll ile
 * güncellenir ve ZigZag / dalga motoru **tam seri** üzerinde çalışır (açık mum dahil). Bu, grafikte
 * güncel fitili göstermek ve intrabar tepkiyi yansıtmak için kasıtlıdır. `repaint === false` yalnızca
 * Pine’daki “yalnız onaylı mum” semantiğini ACP tarama + overlay hizasında uygular; `bars`’ı kısaltmaz.
 *
 * Uygulama: TV `calculated_bars` son N mum; `repaint === false` iken en yeni (muhtemelen açık) mum düşürülür.
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
