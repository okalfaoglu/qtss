import { describe, expect, it } from "vitest";
import { detectBestImpulseV2 } from "./impulse";
import type { ZigzagPivot } from "./types";

function p(index: number, price: number, kind: "high" | "low"): ZigzagPivot {
  return { index, time: index * 60, price, kind };
}

describe("detectBestImpulseV2 hard rule matrix", () => {
  it("accepts a valid bullish standard impulse", () => {
    const pivots: ZigzagPivot[] = [
      p(0, 100, "low"),
      p(1, 120, "high"),
      p(2, 108, "low"),
      p(3, 140, "high"),
      p(4, 126, "low"),
      p(5, 152, "high"),
    ];
    const out = detectBestImpulseV2(pivots, 1, { allowStandard: true, allowDiagonal: false });
    expect(out).not.toBeNull();
    expect(out?.variant).toBe("standard");
  });

  it("rejects when wave2 retraces beyond wave1 start", () => {
    const pivots: ZigzagPivot[] = [
      p(0, 100, "low"),
      p(1, 120, "high"),
      p(2, 98, "low"),
      p(3, 140, "high"),
      p(4, 126, "low"),
      p(5, 152, "high"),
    ];
    const out = detectBestImpulseV2(pivots, 1, { allowStandard: true, allowDiagonal: false });
    expect(out).toBeNull();
  });

  it("rejects when wave3 is shortest among 1-3-5", () => {
    const pivots: ZigzagPivot[] = [
      p(0, 100, "low"),
      p(1, 120, "high"), // |1| = 20
      p(2, 112, "low"),
      p(3, 118, "high"), // |3| = 6 (shortest)
      p(4, 113, "low"),
      p(5, 127, "high"), // |5| = 14
    ];
    const out = detectBestImpulseV2(pivots, 1, { allowStandard: true, allowDiagonal: false });
    expect(out).toBeNull();
  });

  it("rejects when wave3 stays below wave1 end (bull)", () => {
    const pivots: ZigzagPivot[] = [
      p(0, 100, "low"),
      p(1, 120, "high"),
      p(2, 110, "low"),
      p(3, 119, "high"), // P3 < P1
      p(4, 121, "low"), // keep wave4 non-overlap for isolation
      p(5, 133, "high"),
    ];
    const out = detectBestImpulseV2(pivots, 1, { allowStandard: true, allowDiagonal: false });
    expect(out).toBeNull();
  });

  it("rejects overlap for standard but allows it for diagonal", () => {
    const pivots: ZigzagPivot[] = [
      p(0, 100, "low"),
      p(1, 120, "high"),
      p(2, 108, "low"),
      p(3, 140, "high"),
      p(4, 116, "low"), // overlap: P4 <= P1
      p(5, 152, "high"),
    ];
    const standard = detectBestImpulseV2(pivots, 1, { allowStandard: true, allowDiagonal: false });
    expect(standard).toBeNull();

    const diagonal = detectBestImpulseV2(pivots, 1, { allowStandard: false, allowDiagonal: true });
    expect(diagonal).not.toBeNull();
    expect(diagonal?.variant).toBe("diagonal");
  });

  it("rejects when wave3 rises above wave1 end in bearish impulse", () => {
    const pivots: ZigzagPivot[] = [
      p(0, 200, "high"),
      p(1, 180, "low"),
      p(2, 194, "high"),
      p(3, 184, "low"), // should be <= P1 (=180) for bearish rule, but is above
      p(4, 176, "high"), // keep non-overlap valid (P4 < P1) for isolation
      p(5, 160, "low"),
    ];
    const out = detectBestImpulseV2(pivots, 1, { allowStandard: true, allowDiagonal: false });
    expect(out).toBeNull();
  });
});
