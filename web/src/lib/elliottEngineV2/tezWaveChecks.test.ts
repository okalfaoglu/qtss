import { describe, expect, it } from "vitest";
import { TEZ_FLAT_B_VS_A_MAX, TEZ_ZIGZAG_B_VS_A_MAX, buildTez254AbcChecks } from "./tezWaveChecks";
import type { ZigzagPivot } from "./types";

function z(index: number, price: number, kind: "high" | "low"): ZigzagPivot {
  return { index, time: index * 60, price, kind };
}

describe("buildTez254AbcChecks", () => {
  it("flat: flat_r4 and flat_g7 pass inside tez bands", () => {
    const start = z(0, 100, "low");
    const a = z(1, 90, "low");
    const b = z(2, 95, "high");
    const end = z(3, 85, "low");
    const retrB = 0.5;
    const cVsA = 0.5;
    const checks = buildTez254AbcChecks("flat", retrB, cVsA, true, start, a, b, end);
    expect(checks.find((c) => c.id === "flat_r4")?.passed).toBe(true);
    expect(checks.find((c) => c.id === "flat_g7")?.passed).toBe(true);
  });

  it("flat: flat_r4 fails when B/A is outside §2.5.4.1 band", () => {
    const start = z(0, 100, "low");
    const a = z(1, 90, "low");
    const b = z(2, 95, "high");
    const end = z(3, 85, "low");
    const retrB = TEZ_FLAT_B_VS_A_MAX + 0.05;
    const checks = buildTez254AbcChecks("flat", retrB, 0.5, true, start, a, b, end);
    expect(checks.find((c) => c.id === "flat_r4")?.passed).toBe(false);
  });

  it("zigzag: zz_r5 fails when B/A exceeds 61.8%", () => {
    const start = z(0, 100, "low");
    const a = z(1, 90, "low");
    const b = z(2, 94, "high");
    const end = z(3, 75, "low");
    const retrB = TEZ_ZIGZAG_B_VS_A_MAX + 0.02;
    const cVsA = 1.0;
    const checks = buildTez254AbcChecks("zigzag", retrB, cVsA, true, start, a, b, end);
    expect(checks.find((c) => c.id === "zz_r5")?.passed).toBe(false);
  });

  it("zigzag: zz_r5 passes in [0.382, 0.618] with valid C vs B and C beyond A", () => {
    const start = z(0, 100, "low");
    const a = z(1, 90, "low");
    const b = z(2, 95, "high");
    const end = z(3, 75, "low");
    const retrB = 0.5;
    const cVsA = 1.5;
    const checks = buildTez254AbcChecks("zigzag", retrB, cVsA, true, start, a, b, end);
    expect(checks.find((c) => c.id === "zz_r5")?.passed).toBe(true);
    expect(checks.find((c) => c.id === "zz_r1")?.passed).toBe(true);
    expect(checks.find((c) => c.id === "zz_r6")?.passed).toBe(true);
    expect(checks.find((c) => c.id === "zz_b_not_beyond_a_start")?.passed).toBe(true);
  });

  it("zigzag zz_r1: |C| = |B→C|, |B| = |A→B| — C bacağı B’den ölçülür (|a−end| ile karıştırılmaz)", () => {
    const start = z(0, 115, "high");
    const a = z(1, 90, "low");
    const b = z(2, 100, "high");
    const end = z(3, 85, "low");
    const lenA = start.price - a.price;
    const lenB = Math.abs(b.price - a.price);
    const lenC = Math.abs(b.price - end.price);
    expect(lenB).toBe(10);
    expect(lenC).toBe(15);
    const retrB = lenB / lenA;
    const cVsA = lenC / lenA;
    const checks = buildTez254AbcChecks("zigzag", retrB, cVsA, true, start, a, b, end);
    expect(checks.find((c) => c.id === "zz_r5")?.passed).toBe(true);
    expect(checks.find((c) => c.id === "zz_r1")?.passed).toBe(true);
  });

  it("zigzag bear: mirrors B/A and C-end vs A for impulse down (düzeltme yukarı: start low → a high → b low → end high)", () => {
    const start = z(0, 100, "low");
    const a = z(1, 110, "high");
    const b = z(2, 105, "low");
    const end = z(3, 118, "high");
    const retrB = 0.5;
    const cVsA = 0.8;
    const checks = buildTez254AbcChecks("zigzag", retrB, cVsA, false, start, a, b, end);
    expect(checks.find((c) => c.id === "zz_r5")?.passed).toBe(true);
    expect(checks.find((c) => c.id === "zz_r6")?.passed).toBe(true);
  });
});
