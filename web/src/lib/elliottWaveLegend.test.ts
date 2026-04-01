import { describe, expect, it } from "vitest";
import { buildElliottLegendRows } from "./elliottWaveLegend";
import type { ElliottEngineOutputV2, ImpulseCountV2, TimeframeStateV2, ZigzagPivot } from "./elliottEngineV2/types";

function pivot(index: number, price: number, kind: "high" | "low"): ZigzagPivot {
  return { index, time: index * 60, price, kind };
}

function minimalImpulse(): ImpulseCountV2 {
  return {
    direction: "bull",
    variant: "standard",
    pivots: [
      pivot(0, 100, "low"),
      pivot(1, 120, "high"),
      pivot(2, 108, "low"),
      pivot(3, 140, "high"),
      pivot(4, 126, "low"),
      pivot(5, 150, "high"),
    ],
    checks: [{ id: "structure", passed: true }],
    score: 1,
  };
}

function tfState(
  timeframe: TimeframeStateV2["timeframe"],
  decision: TimeframeStateV2["decision"],
  impulse: TimeframeStateV2["impulse"],
): TimeframeStateV2 {
  return {
    timeframe,
    pivots: [],
    impulse,
    wave1NestedImpulse: null,
    wave2NestedCorrective: null,
    wave3NestedImpulse: null,
    wave4NestedCorrective: null,
    wave5NestedImpulse: null,
    wave2: null,
    wave4: null,
    postImpulseAbc: null,
    decision,
  };
}

function engineOut(partial: Partial<ElliottEngineOutputV2["hierarchy"]>): ElliottEngineOutputV2 {
  return {
    states: {},
    hierarchy: {
      macro: partial.macro ?? null,
      intermediate: partial.intermediate ?? null,
      micro: partial.micro ?? null,
    },
    zigzagParams: { depth: 5, deviationPct: 3, backstep: 2 },
  };
}

describe("buildElliottLegendRows", () => {
  it("returns three base rows when output is null", () => {
    const rows = buildElliottLegendRows(null);
    expect(rows).toHaveLength(3);
    expect(rows.map((r) => r.id)).toEqual(["v2_macro", "v2_intermediate", "v2_micro"]);
    expect(rows.every((r) => r.detail.includes("Veri yok"))).toBe(true);
  });

  it("mentions decision when timeframe has no impulse", () => {
    const out = engineOut({
      macro: tfState("4h", "invalid", null),
    });
    const rows = buildElliottLegendRows(out);
    const macro = rows.find((r) => r.id === "v2_macro");
    expect(macro?.detail).toContain("İtki bulunamadı");
    expect(macro?.detail).toContain("invalid");
  });

  it("mentions impulse labels when impulse exists", () => {
    const out = engineOut({
      macro: tfState("4h", "confirmed", minimalImpulse()),
    });
    const rows = buildElliottLegendRows(out);
    const macro = rows.find((r) => r.id === "v2_macro");
    expect(macro?.detail).toContain("①");
    expect(macro?.detail).toContain("confirmed");
  });

  it("appends projection rows when flags are true", () => {
    const rows = buildElliottLegendRows(null, true, true);
    expect(rows.map((r) => r.id)).toEqual([
      "v2_macro",
      "v2_intermediate",
      "v2_micro",
      "projection",
      "projection_targets",
    ]);
  });
});
