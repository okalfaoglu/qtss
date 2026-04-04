import { describe, expect, it } from "vitest";
import { buildElliottProjectionOverlayV2 } from "./projection";
import type {
  CorrectiveCountV2,
  ElliottEngineOutputV2,
  ImpulseCountV2,
  OhlcV2,
  TimeframeStateV2,
  ZigzagPivot,
} from "./types";

function pv(i: number, time: number, price: number, kind: "high" | "low"): ZigzagPivot {
  return { index: i, time, price, kind };
}

function bullImpulse(): ImpulseCountV2 {
  return {
    direction: "bull",
    variant: "standard",
    pivots: [
      pv(0, 1_000_000, 100, "low"),
      pv(1, 1_000_900, 120, "high"),
      pv(2, 1_001_800, 108, "low"),
      pv(3, 1_002_700, 140, "high"),
      pv(4, 1_003_600, 126, "low"),
      pv(5, 1_004_500, 150, "high"),
    ],
    checks: [{ id: "structure", passed: true }],
    score: 1,
  };
}

function microState(imp: ImpulseCountV2): TimeframeStateV2 {
  return {
    timeframe: "15m",
    pivots: [],
    impulse: imp,
    wave1NestedImpulse: null,
    wave2NestedCorrective: null,
    wave3NestedImpulse: null,
    wave4NestedCorrective: null,
    wave5NestedImpulse: null,
    wave2: null,
    wave4: null,
    postImpulseAbc: null,
    decision: "candidate",
  };
}

function engineWithMicro(imp: ImpulseCountV2 | null): ElliottEngineOutputV2 {
  return {
    states: {},
    hierarchy: {
      macro: null,
      intermediate: null,
      micro: imp ? microState(imp) : null,
    },
    zigzagParams: { depth: 5, deviationPct: 3, backstep: 2 },
  };
}

function engine15mWithPost(imp: ImpulseCountV2, post: CorrectiveCountV2): ElliottEngineOutputV2 {
  const st: TimeframeStateV2 = {
    ...microState(imp),
    postImpulseAbc: post,
  };
  return {
    states: { "15m": st },
    hierarchy: { macro: null, intermediate: null, micro: null },
    ohlcByTf: { "15m": anchorTwoBars },
    zigzagParams: { depth: 5, deviationPct: 3, backstep: 2 },
  };
}

const anchorTwoBars: OhlcV2[] = [
  { t: 1_004_400, o: 140, h: 155, l: 135, c: 145 },
  { t: 1_004_500, o: 145, h: 160, l: 144, c: 152 },
];

describe("buildElliottProjectionOverlayV2", () => {
  it("returns null when anchor has fewer than 2 bars", () => {
    const out = buildElliottProjectionOverlayV2(
      engineWithMicro(bullImpulse()),
      [{ t: 1, o: 1, h: 1, l: 1, c: 1 }],
      {},
    );
    expect(out).toBeNull();
  });

  it("returns null when no impulse appears in hierarchy fallback", () => {
    const out = buildElliottProjectionOverlayV2(engineWithMicro(null), anchorTwoBars, {});
    expect(out).toBeNull();
  });

  it("returns null when standard impulse hidden by pattern menu", () => {
    const out = buildElliottProjectionOverlayV2(engineWithMicro(bullImpulse()), anchorTwoBars, {}, {
      motive_impulse: false,
      motive_diagonal_leading: true,
      motive_diagonal_ending: true,
      corrective_zigzag: true,
      corrective_flat: true,
      corrective_triangle: true,
      corrective_complex_double: true,
      corrective_complex_triple: true,
    });
    expect(out).toBeNull();
  });

  it("returns formation projection layers when impulse is visible", () => {
    const out = buildElliottProjectionOverlayV2(engineWithMicro(bullImpulse()), anchorTwoBars, {});
    expect(out).not.toBeNull();
    expect(out!.layers.length).toBeGreaterThan(0);
    expect(out!.layers.some((l) => l.zigzagKind?.includes("elliott_projection"))).toBe(true);
  });

  it("active projection anchors from structural Y (pivots), not path[2], for double W–X–Y", () => {
    const imp: ImpulseCountV2 = {
      direction: "bear",
      variant: "standard",
      pivots: [
        pv(0, 1_000_000, 150, "high"),
        pv(1, 1_000_600, 130, "low"),
        pv(2, 1_001_200, 140, "high"),
        pv(3, 1_001_800, 125, "low"),
        pv(4, 1_002_400, 135, "high"),
        pv(5, 1_003_000, 110, "low"),
      ],
      checks: [{ id: "structure", passed: true }],
      score: 1,
    };
    const p5 = imp.pivots[5]!;
    const w = pv(10, 1_003_200, 125, "high");
    const x = pv(11, 1_003_400, 118, "low");
    const y = pv(12, 1_003_600, 132, "high");
    const pathMid = pv(13, 1_003_500, 128, "high");
    const post: CorrectiveCountV2 = {
      pivots: [p5, w, x, y],
      path: [p5, w, pathMid, x, y],
      labels: ["w", "x", "y"],
      pattern: "combination",
      checks: [],
      score: 0,
    };
    const out = buildElliottProjectionOverlayV2(
      engine15mWithPost(imp, post),
      anchorTwoBars,
      { includeAltScenario: false },
      undefined,
      undefined,
      "15m",
    );
    expect(out).not.toBeNull();
    const active = out!.layers.find((l) => l.zigzagKind === "elliott_projection_c_active");
    expect(active?.zigzag?.[0]?.value).toBe(y.price);
    expect(active?.zigzag?.[0]?.time).toBe(y.time);
  });
});
