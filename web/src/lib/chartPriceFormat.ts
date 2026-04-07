import type { ChartOhlcRow } from "./marketBarsToCandles";

function positiveOhlcSamples(bars: ChartOhlcRow[] | null | undefined): number[] {
  const out: number[] = [];
  if (!bars?.length) return out;
  for (const b of bars) {
    for (const k of ["open", "high", "low", "close"] as const) {
      const v = Number(b[k]);
      if (Number.isFinite(v) && v > 0) out.push(Math.abs(v));
    }
  }
  return out;
}

/**
 * Smallest positive OHLC magnitude in the window — low-priced alts need more scale decimals.
 */
function referencePriceFromBars(bars: ChartOhlcRow[] | null | undefined): number {
  const s = positiveOhlcSamples(bars);
  if (!s.length) return 1;
  return Math.min(...s);
}

/**
 * `lightweight-charts` `priceFormat` for `type: "price"` (right axis ticks, last price label, crosshair).
 */
export function lwcPriceFormatFromOhlcBars(
  bars: ChartOhlcRow[] | null | undefined,
): { type: "price"; precision: number; minMove: number } {
  const ref = referencePriceFromBars(bars);
  return { type: "price", ...lwcTickFromReferencePrice(ref) };
}

export function lwcTickFromReferencePrice(ref: number): { precision: number; minMove: number } {
  const x = Math.abs(ref);
  if (!Number.isFinite(x) || x <= 0) {
    return { precision: 6, minMove: 1e-6 };
  }
  if (x >= 1000) return { precision: 2, minMove: 0.01 };
  if (x >= 1) return { precision: 4, minMove: 1e-4 };
  if (x >= 0.1) return { precision: 5, minMove: 1e-5 };
  if (x >= 0.01) return { precision: 6, minMove: 1e-6 };
  if (x >= 0.0001) return { precision: 8, minMove: 1e-8 };
  if (x >= 1e-5) return { precision: 8, minMove: 1e-8 };
  if (x >= 1e-8) return { precision: 10, minMove: 1e-10 };
  return { precision: 12, minMove: 1e-12 };
}

/** Toolbar / etiket metinleri — eksen ile aynı mertebe. */
export function formatDisplayPrice(n: number): string {
  if (!Number.isFinite(n)) return "—";
  if (n === 0) return "0";
  const a = Math.abs(n);
  const { precision } = lwcTickFromReferencePrice(a);
  return n.toFixed(precision);
}
