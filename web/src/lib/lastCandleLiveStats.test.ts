import { describe, expect, it } from "vitest";
import { lastCandleLiveStatsFromRows } from "./lastCandleLiveStats";
import type { ChartOhlcRow } from "./marketBarsToCandles";

function row(open_time: string, o: string, h: string, l: string, c: string): ChartOhlcRow {
  return { open_time, open: o, high: h, low: l, close: c };
}

describe("lastCandleLiveStatsFromRows", () => {
  it("returns null for empty or missing rows", () => {
    expect(lastCandleLiveStatsFromRows(null)).toBeNull();
    expect(lastCandleLiveStatsFromRows(undefined)).toBeNull();
    expect(lastCandleLiveStatsFromRows([])).toBeNull();
  });

  it("parses last row after chronological sort and computes pctFromOpen", () => {
    const rows = [
      row("2024-01-02T00:00:00Z", "100", "110", "90", "105"),
      row("2024-01-01T00:00:00Z", "1", "1", "1", "1"),
    ];
    const s = lastCandleLiveStatsFromRows(rows);
    expect(s).not.toBeNull();
    expect(s!.open).toBe(100);
    expect(s!.close).toBe(105);
    expect(s!.pctFromOpen).toBeCloseTo(5, 5);
    expect(s!.pctFromPrevClose).toBeCloseTo(10400, 0);
  });

  it("pctFromOpen is null when open is zero", () => {
    const rows = [row("2024-01-01T00:00:00Z", "0", "1", "0", "1")];
    const s = lastCandleLiveStatsFromRows(rows);
    expect(s!.pctFromOpen).toBeNull();
  });

  it("pctFromPrevClose is null when only one row", () => {
    const rows = [row("2024-01-01T00:00:00Z", "10", "11", "9", "10.5")];
    const s = lastCandleLiveStatsFromRows(rows);
    expect(s!.pctFromPrevClose).toBeNull();
  });

  it("returns null when OHLC is not finite", () => {
    const rows = [row("2024-01-01T00:00:00Z", "x", "1", "1", "1")];
    expect(lastCandleLiveStatsFromRows(rows)).toBeNull();
  });

  it("accepts comma decimals", () => {
    const rows = [row("2024-01-01T00:00:00Z", "10", "11", "9", "10,5")];
    const s = lastCandleLiveStatsFromRows(rows);
    expect(s!.close).toBe(10.5);
  });
});
