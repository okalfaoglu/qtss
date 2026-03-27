import type { IChartApi } from "lightweight-charts";

/** Yakınlaştır: görünür mantıksal aralığı daraltır (merkez sabit). */
const ZOOM_IN_FACTOR = 0.82;
/** Uzaklaştır: merkez sabit, aralık genişler. */
const ZOOM_OUT_FACTOR = 1 / ZOOM_IN_FACTOR;
/** Kaydırma: görünür genişliğin bu oranı kadar kaydır. */
const SCROLL_FRAC = 0.35;
const MIN_VISIBLE_BARS = 3;

function logicalRangeOrNull(chart: IChartApi) {
  return chart.timeScale().getVisibleLogicalRange();
}

export function chartZoomIn(chart: IChartApi): void {
  const lr = logicalRangeOrNull(chart);
  if (!lr) return;
  const w = lr.to - lr.from;
  if (w <= MIN_VISIBLE_BARS) return;
  const center = (lr.from + lr.to) / 2;
  const newHalf = (w / 2) * ZOOM_IN_FACTOR;
  chart.timeScale().setVisibleLogicalRange({ from: center - newHalf, to: center + newHalf });
}

export function chartZoomOut(chart: IChartApi): void {
  const lr = logicalRangeOrNull(chart);
  if (!lr) return;
  const w = lr.to - lr.from;
  const center = (lr.from + lr.to) / 2;
  const newHalf = (w / 2) * ZOOM_OUT_FACTOR;
  chart.timeScale().setVisibleLogicalRange({ from: center - newHalf, to: center + newHalf });
}

/** Eski barlara (sol / geçmiş). */
export function chartScrollLeft(chart: IChartApi): void {
  const lr = logicalRangeOrNull(chart);
  if (!lr) return;
  const w = lr.to - lr.from;
  const delta = w * SCROLL_FRAC;
  chart.timeScale().setVisibleLogicalRange({ from: lr.from - delta, to: lr.to - delta });
}

/** Yeni barlara (sağ / güncel). */
export function chartScrollRight(chart: IChartApi): void {
  const lr = logicalRangeOrNull(chart);
  if (!lr) return;
  const w = lr.to - lr.from;
  const delta = w * SCROLL_FRAC;
  chart.timeScale().setVisibleLogicalRange({ from: lr.from + delta, to: lr.to + delta });
}

/** Tüm seriyi sığdır (TV “reset” / ölçek sıfırlama). */
export function chartResetView(chart: IChartApi): void {
  chart.timeScale().fitContent();
}
