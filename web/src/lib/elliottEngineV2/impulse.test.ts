import { describe, expect, it } from "vitest";
import { detectBestImpulseV2, detectHistoricalImpulsesV2 } from "./impulse";
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
    // Diyagonal §2.5.3.4: ed_r4, w5_not_longest, ld_r3 (|5|≥1.38|4|) ile uyumlu fiyatlar
    const pivots: ZigzagPivot[] = [
      p(0, 100, "low"),
      p(1, 120, "high"),
      p(2, 108, "low"),
      p(3, 140, "high"),
      p(4, 117, "low"), // |4|=23 → |5|≥~31.74; |5|=32, w5 en uzun değil (|3|=32)
      p(5, 149, "high"),
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

  it("rejects when wave2 is longer than wave1 (bull)", () => {
    const pivots: ZigzagPivot[] = [
      p(0, 100, "low"),
      p(1, 120, "high"), // |1| = 20
      p(2, 95, "low"), // |2| = 25 > |1|
      p(3, 140, "high"),
      p(4, 126, "low"),
      p(5, 152, "high"),
    ];
    expect(detectBestImpulseV2(pivots, 1, { allowStandard: true, allowDiagonal: false })).toBeNull();
  });

  it("accepts a valid bearish standard impulse", () => {
    const pivots: ZigzagPivot[] = [
      p(0, 200, "high"),
      p(1, 180, "low"),
      p(2, 192, "high"),
      p(3, 172, "low"),
      p(4, 175, "high"), // P4 < P1 (180): standart w4–w1 ayrımı
      p(5, 160, "low"),
    ];
    const out = detectBestImpulseV2(pivots, 1, { allowStandard: true, allowDiagonal: false });
    expect(out).not.toBeNull();
    expect(out?.direction).toBe("bear");
    expect(out?.variant).toBe("standard");
  });

  it("rejects bearish when wave2 retraces beyond wave1 start", () => {
    const pivots: ZigzagPivot[] = [
      p(0, 200, "high"),
      p(1, 180, "low"),
      p(2, 205, "high"), // P2 > P0
      p(3, 172, "low"),
      p(4, 175, "high"),
      p(5, 160, "low"),
    ];
    expect(detectBestImpulseV2(pivots, 1, { allowStandard: true, allowDiagonal: false })).toBeNull();
  });

  it("rejects bearish standard on w4–w1 overlap but allows bearish diagonal", () => {
    // P4=185 > P1=180 → standart fail; diyagonal: ld_r3 / ed_r4 / w5_not_longest ile uyumlu (P5=165)
    const pivots: ZigzagPivot[] = [
      p(0, 200, "high"),
      p(1, 180, "low"),
      p(2, 192, "high"),
      p(3, 172, "low"),
      p(4, 185, "high"),
      p(5, 165, "low"),
    ];
    expect(detectBestImpulseV2(pivots, 1, { allowStandard: true, allowDiagonal: false })).toBeNull();

    const diag = detectBestImpulseV2(pivots, 1, { allowStandard: false, allowDiagonal: true });
    expect(diag).not.toBeNull();
    expect(diag?.direction).toBe("bear");
    expect(diag?.variant).toBe("diagonal");
  });

  it("detectHistoricalImpulsesV2 returns at least one non-overlapping hit for a long pivot run", () => {
    const pivots: ZigzagPivot[] = [
      p(0, 100, "low"),
      p(1, 120, "high"),
      p(2, 108, "low"),
      p(3, 140, "high"),
      p(4, 126, "low"),
      p(5, 152, "high"),
      p(6, 130, "low"),
      p(7, 160, "high"),
      p(8, 145, "low"),
      p(9, 175, "high"),
      p(10, 162, "low"), // P4 > P1 (160): standart w4–w1 ayrımı
      p(11, 188, "high"),
    ];
    const hist = detectHistoricalImpulsesV2(pivots, 80, 8, {
      allowStandard: true,
      allowDiagonal: false,
    });
    expect(hist.length).toBeGreaterThanOrEqual(1);
    for (let i = 1; i < hist.length; i++) {
      const a = hist[i - 1]!.pivots[5].index;
      const b = hist[i]!.pivots[0].index;
      expect(b > a).toBe(true);
    }
  });
});
