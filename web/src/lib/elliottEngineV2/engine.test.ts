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

function baseBearImpulse(): ImpulseCountV2 {
  return {
    direction: "bear",
    variant: "standard",
    pivots: [
      pivot(0, 200, "high"),
      pivot(1, 180, "low"),
      pivot(2, 192, "high"),
      pivot(3, 172, "low"),
      pivot(4, 175, "high"),
      pivot(5, 160, "low"),
    ],
    checks: [
      { id: "structure", passed: true },
      { id: "w2_not_beyond_w1_start", passed: true },
      { id: "w2_not_longer_than_w1", passed: true },
      { id: "w3_not_shortest_135", passed: true },
      { id: "w3_not_above_w1_end", passed: true },
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

/** Tez + nested A/C motive proof (see `engine.ts` addZigzagMotiveProof). */
const zigzagConfirmedChecks: CorrectiveCountV2["checks"] = [
  { id: "abc_order", passed: true },
  { id: "zz_r1", passed: true },
  { id: "zz_r5", passed: true },
  { id: "zz_r6", passed: true },
  { id: "zz_a_motive5", passed: true },
  { id: "zz_c_motive5", passed: true },
];

/** Tez + nested A/B corrective + C motive proof (see `engine.ts` addFlatStructureProof). */
const flatConfirmedChecks: CorrectiveCountV2["checks"] = [
  { id: "abc_order", passed: true },
  { id: "flat_r4", passed: true },
  { id: "flat_g7", passed: true },
  { id: "flat_a_corrective3", passed: true },
  { id: "flat_b_corrective3", passed: true },
  { id: "flat_c_motive5", passed: true },
];

const diagonalEndingConfirmedChecks: ImpulseCountV2["checks"] = [
  { id: "diag_1_corrective3", passed: true },
  { id: "diag_2_corrective3", passed: true },
  { id: "diag_3_corrective3", passed: true },
  { id: "diag_4_corrective3", passed: true },
  { id: "diag_5_corrective3", passed: true },
];

const diagonalLeadingConfirmedChecks: ImpulseCountV2["checks"] = [
  { id: "diag_ld_1_motive5", passed: true },
  { id: "diag_ld_2_corr3", passed: true },
  { id: "diag_ld_3_motive5", passed: true },
  { id: "diag_ld_4_corr3", passed: true },
  { id: "diag_ld_5_motive5", passed: true },
];

/** Tez + nested corrective-3 per leg A–E (see `engine.ts` addTriangleLegProof). */
const triangleConfirmedChecks: CorrectiveCountV2["checks"] = [
  { id: "tri_r5", passed: true },
  { id: "tri_r2_channel", passed: true },
  { id: "tri_r3_apex_after_e", passed: true },
  { id: "tri_r4_not_parallel", passed: true },
  { id: "triangle_converging", passed: true },
  { id: "triangle_envelope_contract", passed: true },
  { id: "tri_a_corrective3", passed: true },
  { id: "tri_b_corrective3", passed: true },
  { id: "tri_c_corrective3", passed: true },
  { id: "tri_d_corrective3", passed: true },
  { id: "tri_e_corrective3", passed: true },
];

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
    const wave2 = corr("zigzag", zigzagConfirmedChecks);
    expect(decideTimeframeState(state({ wave2 }))).toBe("confirmed");
  });

  it("keeps candidate for zigzag when a required tez check is missing", () => {
    const wave2 = corr("zigzag", [
      ...zigzagConfirmedChecks.map((x) => (x.id === "zz_r6" ? { ...x, passed: false } : x)),
    ]);
    expect(decideTimeframeState(state({ wave2 }))).toBe("candidate");
  });

  it("keeps candidate for zigzag when abc_order fails", () => {
    const wave2 = corr("zigzag", [
      ...zigzagConfirmedChecks.map((x) => (x.id === "abc_order" ? { ...x, passed: false } : x)),
    ]);
    expect(decideTimeframeState(state({ wave2 }))).toBe("candidate");
  });

  it("returns confirmed for flat with required checks", () => {
    const wave4 = corr("flat", flatConfirmedChecks);
    expect(decideTimeframeState(state({ wave4 }))).toBe("confirmed");
  });

  it("keeps candidate for flat when flat_r4 or flat_g7 fails", () => {
    const wave4 = corr("flat", [
      ...flatConfirmedChecks.map((x) => (x.id === "flat_r4" ? { ...x, passed: false } : x)),
    ]);
    expect(decideTimeframeState(state({ wave4 }))).toBe("candidate");
    const wave4b = corr("flat", [
      ...flatConfirmedChecks.map((x) => (x.id === "flat_g7" ? { ...x, passed: false } : x)),
    ]);
    expect(decideTimeframeState(state({ wave4: wave4b }))).toBe("candidate");
  });

  it("keeps candidate for flat when abc_order fails", () => {
    const wave4 = corr("flat", [
      { id: "abc_order", passed: false },
      { id: "flat_r4", passed: true },
      { id: "flat_g7", passed: true },
    ]);
    expect(decideTimeframeState(state({ wave4 }))).toBe("candidate");
  });

  it("returns confirmed for triangle with required checks", () => {
    const wave2 = corr("triangle", triangleConfirmedChecks);
    expect(decideTimeframeState(state({ wave2 }))).toBe("confirmed");
  });

  it("returns confirmed for expanding triangle when expand checks pass", () => {
    const wave2 = corr("triangle", [
      { id: "tri_r5", passed: true },
      { id: "tri_r2_channel", passed: true },
      { id: "tri_r3_apex_after_e", passed: true },
      { id: "tri_r4_not_parallel", passed: true },
      { id: "triangle_converging", passed: false },
      { id: "triangle_envelope_contract", passed: false },
      { id: "triangle_expanding", passed: true },
      { id: "tri_r7_expanding_shortest_ab", passed: true },
      { id: "tri_a_corrective3", passed: true },
      { id: "tri_b_corrective3", passed: true },
      { id: "tri_c_corrective3", passed: true },
      { id: "tri_d_corrective3", passed: true },
      { id: "tri_e_corrective3", passed: true },
    ]);
    expect(decideTimeframeState(state({ wave2 }))).toBe("confirmed");
  });

  it("keeps candidate for triangle when a required structural check fails", () => {
    const wave2 = corr("triangle", [
      ...triangleConfirmedChecks.map((x) =>
        x.id === "triangle_converging" ? { ...x, passed: false } : x,
      ),
    ]);
    expect(decideTimeframeState(state({ wave2 }))).toBe("candidate");
  });

  it("keeps candidate for triangle when a nested leg check fails", () => {
    const wave2 = corr("triangle", [
      ...triangleConfirmedChecks.map((x) =>
        x.id === "tri_c_corrective3" ? { ...x, passed: false } : x,
      ),
    ]);
    expect(decideTimeframeState(state({ wave2 }))).toBe("candidate");
  });

  it("returns candidate when one internal is confirmed but the other is not", () => {
    const wave2 = corr("zigzag", zigzagConfirmedChecks);
    const wave4 = corr("flat", [
      ...flatConfirmedChecks.map((x) => (x.id === "flat_r4" ? { ...x, passed: false } : x)),
    ]);
    expect(decideTimeframeState(state({ wave2, wave4 }))).toBe("candidate");
  });

  it("returns confirmed only when both wave2 and wave4 are confirmed", () => {
    const wave2 = corr("zigzag", zigzagConfirmedChecks);
    const wave4 = corr("flat", flatConfirmedChecks);
    expect(decideTimeframeState(state({ wave2, wave4 }))).toBe("confirmed");
  });

  it("ignores post-impulse ABC for confirmed (only wave2 / wave4 internals)", () => {
    const post = corr("zigzag", zigzagConfirmedChecks);
    expect(
      decideTimeframeState(
        state({
          impulse: baseImpulse(),
          postImpulseAbc: post,
          wave2: null,
          wave4: null,
        }),
      ),
    ).toBe("candidate");
  });

  it("keeps candidate for combination without W–X–Y or WXYXZ confirmation", () => {
    const wave4 = corr("combination", [
      { id: "comb_confirmed", passed: false },
      { id: "wxyxz_confirmed", passed: false },
    ]);
    expect(decideTimeframeState(state({ wave4 }))).toBe("candidate");
  });

  it("returns confirmed for combination when WXYXZ confirmed", () => {
    const wave4 = corr("combination", [{ id: "wxyxz_confirmed", passed: true }]);
    expect(decideTimeframeState(state({ wave4 }))).toBe("confirmed");
  });

  it("returns confirmed for combination when W–X–Y comb_confirmed", () => {
    const wave4 = corr("combination", [{ id: "comb_confirmed", passed: true }]);
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

  it("keeps candidate for diagonal impulse when diagonal subwave proof is missing", () => {
    const imp = baseImpulse("diagonal");
    const wave2 = corr("zigzag", zigzagConfirmedChecks);
    expect(decideTimeframeState(state({ impulse: imp, wave2 }))).toBe("candidate");
  });

  it("allows confirmed for diagonal impulse when diagonal subwave proof is present", () => {
    const imp = baseImpulse("diagonal");
    imp.checks = [...imp.checks, ...diagonalEndingConfirmedChecks];
    const wave2 = corr("zigzag", zigzagConfirmedChecks);
    expect(decideTimeframeState(state({ impulse: imp, wave2 }))).toBe("confirmed");
  });

  it("keeps candidate for leading diagonal when ending-style subwave proof is used", () => {
    const imp = baseImpulse("diagonal");
    imp.diagonalRole = "leading";
    imp.checks = [...imp.checks, ...diagonalEndingConfirmedChecks];
    const wave2 = corr("zigzag", zigzagConfirmedChecks);
    expect(decideTimeframeState(state({ impulse: imp, wave2 }))).toBe("candidate");
  });

  it("allows confirmed for leading diagonal when leading-style subwave proof is present", () => {
    const imp = baseImpulse("diagonal");
    imp.diagonalRole = "leading";
    imp.checks = [...imp.checks, ...diagonalLeadingConfirmedChecks];
    const wave2 = corr("zigzag", zigzagConfirmedChecks);
    expect(decideTimeframeState(state({ impulse: imp, wave2 }))).toBe("confirmed");
  });

  it("invalidates when a hard impulse check fails", () => {
    const imp = baseImpulse("standard");
    imp.checks = imp.checks.map((c) => (c.id === "w3_not_shortest_135" ? { ...c, passed: false } : c));
    expect(decideTimeframeState(state({ impulse: imp }))).toBe("invalid");
  });

  it("invalidates when w2_not_beyond_w1_start fails (bull)", () => {
    const imp = baseImpulse("standard");
    imp.checks = imp.checks.map((c) => (c.id === "w2_not_beyond_w1_start" ? { ...c, passed: false } : c));
    expect(decideTimeframeState(state({ impulse: imp }))).toBe("invalid");
  });

  it("invalidates bearish impulse when w3_not_above_w1_end fails", () => {
    const imp = baseBearImpulse();
    imp.checks = imp.checks.map((c) => (c.id === "w3_not_above_w1_end" ? { ...c, passed: false } : c));
    expect(decideTimeframeState(state({ impulse: imp }))).toBe("invalid");
  });
});
