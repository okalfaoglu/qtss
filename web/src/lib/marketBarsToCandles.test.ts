import { describe, expect, it } from "vitest";
import { marketBarsToCandles } from "./marketBarsToCandles";
import type { ChartOhlcRow } from "./marketBarsToCandles";

function row(open_time: string, o: number, h: number, l: number, c: number): ChartOhlcRow {
  return { open_time, open: o, high: h, low: l, close: c };
}

describe("marketBarsToCandles", () => {
  it("returns empty for null, undefined, or empty input", () => {
    expect(marketBarsToCandles(null)).toEqual([]);
    expect(marketBarsToCandles(undefined)).toEqual([]);
    expect(marketBarsToCandles([])).toEqual([]);
  });

  it("deduplicates same-second open_time keeping last row", () => {
    const rows: ChartOhlcRow[] = [
      row("2024-01-01T00:00:00.100Z", 1, 2, 0.5, 1.5),
      row("2024-01-01T00:00:00.500Z", 10, 11, 9, 10),
    ];
    const candles = marketBarsToCandles(rows);
    expect(candles).toHaveLength(1);
    expect(candles[0].time).toBe(Math.floor(new Date("2024-01-01T00:00:00.000Z").getTime() / 1000));
    expect(candles[0].close).toBe(10);
  });

  it("sorts candles by time ascending", () => {
    const rows = [
      row("2024-01-02T00:00:00Z", 1, 1, 1, 1),
      row("2024-01-01T00:00:00Z", 2, 2, 2, 2),
    ];
    const candles = marketBarsToCandles(rows);
    expect(candles.map((c) => c.open)).toEqual([2, 1]);
  });
});
