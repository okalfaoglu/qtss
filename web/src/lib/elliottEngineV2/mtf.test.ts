import { describe, expect, it } from "vitest";
import { buildMtfFramesV2 } from "./mtf";
import type { OhlcV2, Timeframe } from "./types";

function bar(t: number, c: number): OhlcV2 {
  return { t, o: c, h: c + 0.5, l: c - 0.5, c };
}

describe("buildMtfFramesV2", () => {
  it("returns empty object when anchor empty", () => {
    expect(buildMtfFramesV2([], "15m")).toEqual({});
  });

  it("sorts anchor by time and attaches under anchor Tf key", () => {
    const rows = [bar(200, 2), bar(100, 1)];
    const out = buildMtfFramesV2(rows, "4h");
    expect(out["4h"]!.map((r) => r.t)).toEqual([100, 200]);
  });

  it("15m anchor yields 1h and 4h aggregates", () => {
    const sec15 = 15 * 60;
    const rows: OhlcV2[] = [0, 1, 2, 3].map((i) => bar(i * sec15, 10 + i));
    const out = buildMtfFramesV2(rows, "15m");
    expect(Object.keys(out).sort() as Timeframe[]).toEqual(["15m", "1h", "4h"]);
    expect(out["15m"]!.length).toBe(4);
    expect(out["1h"]!.length).toBe(1);
    expect(out["4h"]!.length).toBe(1);
    const h = out["1h"]![0];
    // bar() uses low = close - 0.5 per row; aggregate low is min of those (9.5 for 10..13 closes).
    expect(h.l).toBe(9.5);
    expect(h.h).toBe(13.5);
    expect(h.c).toBe(13);
  });

  it("1h anchor adds only 4h bucket series", () => {
    const h = 60 * 60;
    const rows: OhlcV2[] = [bar(0, 1), bar(h, 2), bar(2 * h, 3)];
    const out = buildMtfFramesV2(rows, "1h");
    expect(Object.keys(out).sort()).toEqual(["1h", "4h"]);
    expect(out["4h"]!.length).toBe(1);
  });

  it("4h anchor only exposes 4h frame", () => {
    const rows = [bar(0, 1), bar(4 * 60 * 60, 2)];
    const out = buildMtfFramesV2(rows, "4h");
    expect(Object.keys(out)).toEqual(["4h"]);
  });
});
