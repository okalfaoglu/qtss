import { describe, expect, it } from "vitest";
import { mergeChartOhlcRowsByOpenTime } from "./mergeChartOhlcRows";
import type { ChartOhlcRow } from "./marketBarsToCandles";

function row(open_time: string, open: string): ChartOhlcRow {
  return { open_time, open, high: open, low: open, close: open };
}

describe("mergeChartOhlcRowsByOpenTime", () => {
  it("returns chronologically sorted copy when delta is empty", () => {
    const existing = [row("2024-01-02T00:00:00Z", "2"), row("2024-01-01T00:00:00Z", "1")];
    const out = mergeChartOhlcRowsByOpenTime(existing, []);
    expect(out.map((r) => r.open)).toEqual(["1", "2"]);
  });

  it("replaces row at same instant when ISO formatting differs", () => {
    const existing = [row("2024-01-01T00:00:00.000Z", "old")];
    const delta = [row("2024-01-01T00:00:00Z", "new")];
    const out = mergeChartOhlcRowsByOpenTime(existing, delta);
    expect(out).toHaveLength(1);
    expect(out[0].open).toBe("new");
  });

  it("delta overrides existing open_time key", () => {
    const existing = [
      row("2024-01-01T00:00:00Z", "a"),
      row("2024-01-02T00:00:00Z", "b"),
    ];
    const delta = [row("2024-01-02T00:00:00Z", "updated")];
    const out = mergeChartOhlcRowsByOpenTime(existing, delta);
    expect(out.map((r) => ({ t: r.open_time, o: r.open }))).toEqual([
      { t: "2024-01-01T00:00:00Z", o: "a" },
      { t: "2024-01-02T00:00:00Z", o: "updated" },
    ]);
  });

  it("appends new bars from delta", () => {
    const existing = [row("2024-01-01T00:00:00Z", "1")];
    const delta = [row("2024-01-02T00:00:00Z", "2")];
    const out = mergeChartOhlcRowsByOpenTime(existing, delta);
    expect(out.map((r) => r.open)).toEqual(["1", "2"]);
  });
});
