import { describe, expect, it } from "vitest";
import {
  formatDisplayPrice,
  lwcPriceFormatFromOhlcBars,
  lwcTickFromReferencePrice,
} from "./chartPriceFormat";
import type { ChartOhlcRow } from "./marketBarsToCandles";

function row(p: number): ChartOhlcRow {
  const t = new Date().toISOString();
  const s = (x: number) => String(x);
  return {
    open_time: t,
    open: s(p),
    high: s(p * 1.01),
    low: s(p * 0.99),
    close: s(p),
    volume: "0",
  };
}

describe("lwcTickFromReferencePrice", () => {
  it("uses 6 decimals for ~0.018 alt prices", () => {
    const f = lwcTickFromReferencePrice(0.01837);
    expect(f.precision).toBe(6);
    expect(f.minMove).toBe(1e-6);
  });

  it("uses 4 decimals for >= 1", () => {
    const f = lwcTickFromReferencePrice(42);
    expect(f.precision).toBe(4);
  });

  it("uses 8 decimals for sub-cent meme range", () => {
    const f = lwcTickFromReferencePrice(0.000052);
    expect(f.precision).toBe(8);
  });
});

describe("lwcPriceFormatFromOhlcBars", () => {
  it("derives from minimum positive OHLC", () => {
    const bars: ChartOhlcRow[] = [row(0.5), row(0.018), row(0.019)];
    const f = lwcPriceFormatFromOhlcBars(bars);
    expect(f.precision).toBe(6);
    expect(f.type).toBe("price");
  });
});

describe("formatDisplayPrice", () => {
  it("matches magnitude of small prices", () => {
    expect(formatDisplayPrice(0.01837)).toBe("0.018370");
  });
});
