import { describe, expect, it } from "vitest";
import { chartOhlcRowsSortedChrono, chartOhlcRowsToScanBars } from "./chartRowsToOhlcBars";
import type { ChartOhlcRow } from "./marketBarsToCandles";

function row(open_time: string, o: string, h: string, l: string, c: string): ChartOhlcRow {
  return { open_time, open: o, high: h, low: l, close: c };
}

describe("chartOhlcRowsSortedChrono", () => {
  it("sorts by open_time ascending; stable tie-break by input order", () => {
    const a = row("2024-01-02T00:00:00.000Z", "1", "2", "0.5", "1.5");
    const b = row("2024-01-01T00:00:00.000Z", "10", "11", "9", "10.5");
    const c = row("2024-01-01T00:00:00.000Z", "100", "101", "99", "100.5");
    const out = chartOhlcRowsSortedChrono([a, b, c]);
    expect(out.map((r) => r.open)).toEqual(["10", "100", "1"]);
  });

  it("accepts comma decimals in OHLC strings", () => {
    const r = row("2024-06-01T12:00:00Z", "1,5", "2,25", "1,0", "2");
    const bars = chartOhlcRowsToScanBars([r]);
    expect(bars).toEqual([
      { bar_index: 0, open: 1.5, high: 2.25, low: 1, close: 2 },
    ]);
  });

  it("maps numeric OHLC fields", () => {
    const r = {
      open_time: "2024-06-01T12:00:00Z",
      open: 1,
      high: 2,
      low: 0.5,
      close: 1.75,
    } as ChartOhlcRow;
    const bars = chartOhlcRowsToScanBars([r]);
    expect(bars[0]).toMatchObject({ bar_index: 0, open: 1, high: 2, low: 0.5, close: 1.75 });
  });
});

describe("chartOhlcRowsToScanBars", () => {
  it("assigns bar_index 0..n-1 after chronological sort", () => {
    const rows = [
      row("2024-01-03T00:00:00Z", "1", "1", "1", "1"),
      row("2024-01-01T00:00:00Z", "2", "2", "2", "2"),
      row("2024-01-02T00:00:00Z", "3", "3", "3", "3"),
    ];
    const bars = chartOhlcRowsToScanBars(rows);
    expect(bars.map((b) => b.bar_index)).toEqual([0, 1, 2]);
    expect(bars.map((b) => b.open)).toEqual([2, 3, 1]);
  });
});
