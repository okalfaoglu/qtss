import { describe, expect, it } from "vitest";
import { buildChannelScanPivotMarkers } from "./channelScanMarkers";
import type { ChartOhlcRow } from "./marketBarsToCandles";

function bar(iso: string): ChartOhlcRow {
  return { open_time: iso, open: "1", high: "2", low: "0.5", close: "1" };
}

describe("buildChannelScanPivotMarkers", () => {
  const bars = [
    bar("2024-01-01T00:00:00Z"),
    bar("2024-01-01T00:15:00Z"),
    bar("2024-01-01T00:30:00Z"),
  ];

  it("skips out-of-range bar_index", () => {
    const m = buildChannelScanPivotMarkers(bars, [[-1, 1, 1], [99, 1, -1], [1, 10, 1]], "dark");
    expect(m).toHaveLength(1);
    expect(m[0].time).toBe(Math.floor(new Date(bars[1].open_time).getTime() / 1000));
  });

  it("maps dir > 0 to peak (H) and dir <= 0 to trough (L)", () => {
    const m = buildChannelScanPivotMarkers(bars, [[0, 1, 1], [2, 1, -1]], "dark");
    expect(m[0].text).toBe("H");
    expect(m[0].position).toBe("aboveBar");
    expect(m[1].text).toBe("L");
    expect(m[1].position).toBe("belowBar");
  });

  it("uses light theme colors when theme is light", () => {
    const dark = buildChannelScanPivotMarkers(bars, [[0, 1, 1]], "dark")[0].color;
    const light = buildChannelScanPivotMarkers(bars, [[0, 1, 1]], "light")[0].color;
    expect(dark).toBe("#26a69a");
    expect(light).toBe("#089981");
  });

  it("sorts markers by time ascending", () => {
    const m = buildChannelScanPivotMarkers(bars, [[2, 1, 1], [0, 1, -1], [1, 1, 1]], "dark");
    const times = m.map((x) => x.time as number);
    expect(times).toEqual([...times].sort((a, b) => a - b));
  });
});
