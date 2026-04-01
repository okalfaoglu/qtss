import { describe, expect, it } from "vitest";
import { acpOhlcWindowForScan } from "./acpScanWindow";
import type { ChartOhlcRow } from "./marketBarsToCandles";

function row(open_time: string, v: string): ChartOhlcRow {
  return { open_time, open: v, high: v, low: v, close: v };
}

describe("acpOhlcWindowForScan", () => {
  it("returns empty for null, undefined, or empty bars", () => {
    expect(acpOhlcWindowForScan(null, 100, true)).toEqual([]);
    expect(acpOhlcWindowForScan(undefined, 100, true)).toEqual([]);
    expect(acpOhlcWindowForScan([], 100, true)).toEqual([]);
  });

  it("sorts chronologically then takes last calculatedBars capped by length", () => {
    const bars = [
      row("2024-01-03T00:00:00Z", "3"),
      row("2024-01-01T00:00:00Z", "1"),
      row("2024-01-02T00:00:00Z", "2"),
    ];
    const w = acpOhlcWindowForScan(bars, 2, true);
    expect(w.map((r) => r.open)).toEqual(["2", "3"]);
  });

  it("clamps calculatedBars to at least 1", () => {
    const bars = [row("2024-01-01T00:00:00Z", "a"), row("2024-01-02T00:00:00Z", "b")];
    expect(acpOhlcWindowForScan(bars, 0, true)).toEqual([bars[1]]);
  });

  it("with repaint false drops the newest bar when more than one in window", () => {
    const bars = [
      row("2024-01-01T00:00:00Z", "1"),
      row("2024-01-02T00:00:00Z", "2"),
      row("2024-01-03T00:00:00Z", "3"),
    ];
    const w = acpOhlcWindowForScan(bars, 3, false);
    expect(w.map((r) => r.open)).toEqual(["1", "2"]);
  });

  it("with repaint false and single bar keeps that bar", () => {
    const bars = [row("2024-01-01T00:00:00Z", "only")];
    expect(acpOhlcWindowForScan(bars, 5, false)).toEqual([bars[0]]);
  });
});
