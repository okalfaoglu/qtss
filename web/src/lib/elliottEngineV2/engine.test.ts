import { describe, expect, it } from "vitest";
import { decideTimeframeState } from "./engine";
import type { CorrectiveCountV2, ImpulseCountV2, TimeframeStateV2, ZigzagPivot } from "./types";

function pivot(index: number, price: number, kind: "high" | "low"): ZigzagPivot {
  return { index, time: index * 60, price, kind };
}

function baseImpulse(variant: "standard" | "diagonal" = "standard"): ImpulseCountV2 {
  return {
    direction: "bull",
    variant,
    pivots: [
      pivot(0, 100, "low"),
      pivot(1, 120, "high"),
      pivot(2, 108, "low"),
      pivot(3, 140, "high"),
      pivot(4, 126, "low"),
      pivot(5, 150, "high"),
    ],
    checks: [
      { id: "structure", passed: true },
      { id: "w2_not_beyond_w1_start", passed: true },
      { id: "w2_not_longer_than_w1", passed: true },
      { id: "w3_not_shortest_135", passed: true },
      { id: "w3_not_below_w1_end", passed: true },
      { id: "w4_no_overlap_w1", passed: true },
    ],
    score: 6,
  };
}

function corr(pattern: CorrectiveCountV2["pattern"], checks: CorrectiveCountV2["checks"]): CorrectiveCountV2 {
  return {
    pivots: [pivot(1, 120, "high"), pivot(2, 110, "low"), pivot(3, 116, "high"), pivot(4, 106, "low")],
    pattern,
    checks,
    score: checks.filter((x) => x.passed).length,
  };
}

function state(overrides: Partial<Omit<TimeframeStateV2, "decision">>): Omit<TimeframeStateV2, "decision"> {
  return {
    timeframe: "15m",
    pivots: [],
    impulse: baseImpulse(),
    historicalImpulses: [],
    wave2: null,
    wave4: null,
    postImpulseAbc: null,
    ...overrides,
  };
}

describe("decideTimeframeState", () => {
  it("returns invalid when no impulse", () => {
    expect(decideTimeframeState(state({ impulse: null }))).toBe("invalid");
  });

  it("returns candidate when impulse exists but no internal corrections", () => {
    expect(decideTimeframeState(state({}))).toBe("candidate");
  });

  it("returns confirmed for zigzag with required checks", () => {
    const wave2 = corr("zigzag", [
      { id: "abc_order", passed: true },
      { id: "zz_r1", passed: true },
      { id: "zz_r5", passed: true },
      { id: "zz_r6", passed: true },
    ]);
    expect(decideTimeframeState(state({ wave2 }))).toBe("confirmed");
  });

  it("keeps candidate for combination without confirmed WXYXZ", () => {
    const wave4 = corr("combination", [
      { id: "wxyxz_confirmed", passed: false },
      { id: "wxyxz_alt", passed: true },
    ]);
    expect(decideTimeframeState(state({ wave4 }))).toBe("candidate");
  });

  it("returns confirmed for combination when WXYXZ confirmed", () => {
    const wave4 = corr("combination", [{ id: "wxyxz_confirmed", passed: true }]);
    expect(decideTimeframeState(state({ wave4 }))).toBe("confirmed");
  });

  it("invalidates standard impulse when wave4 overlaps wave1", () => {
    const imp = baseImpulse("standard");
    imp.checks = imp.checks.map((c) => (c.id === "w4_no_overlap_w1" ? { ...c, passed: false } : c));
    expect(decideTimeframeState(state({ impulse: imp }))).toBe("invalid");
  });

  it("does not hard-invalidate diagonal on wave4 overlap check", () => {
    const imp = baseImpulse("diagonal");
    imp.checks = imp.checks.map((c) => (c.id === "w4_no_overlap_w1" ? { ...c, passed: false } : c));
    expect(decideTimeframeState(state({ impulse: imp }))).toBe("candidate");
  });

  it("invalidates when a hard impulse check fails", () => {
    const imp = baseImpulse("standard");
    imp.checks = imp.checks.map((c) => (c.id === "w3_not_shortest_135" ? { ...c, passed: false } : c));
    expect(decideTimeframeState(state({ impulse: imp }))).toBe("invalid");
  });
});
