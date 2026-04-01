import { describe, expect, it } from "vitest";
import { CHART_INTERVALS, type ChartInterval } from "./chartIntervals";

describe("CHART_INTERVALS", () => {
  it("lists Binance-style intervals in ascending rough order for UI", () => {
    expect(CHART_INTERVALS[0]).toBe("1m");
    expect(CHART_INTERVALS).toContain("15m");
    expect(CHART_INTERVALS).toContain("1h");
    expect(CHART_INTERVALS).toContain("1M");
  });

  it("has unique entries", () => {
    expect(new Set(CHART_INTERVALS).size).toBe(CHART_INTERVALS.length);
  });

  it("ChartInterval is assignable for known members", () => {
    const x: ChartInterval = "4h";
    expect(x).toBe("4h");
  });
});
