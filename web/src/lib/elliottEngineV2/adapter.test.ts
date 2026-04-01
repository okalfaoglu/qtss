import { describe, expect, it } from "vitest";
import { correctiveLabelAnchors } from "./adapter";
import type { CorrectiveCountV2, ZigzagPivot } from "./types";

function p(i: number, kind: "high" | "low"): ZigzagPivot {
  return { index: i, time: i * 1000, price: 100 + i, kind };
}

describe("correctiveLabelAnchors", () => {
  it("uses pivots.slice(1) for double W–X–Y when path has extra zigzag vertices", () => {
    const start = p(0, "low");
    const wEnd = p(1, "high");
    const xEnd = p(2, "low");
    const end = p(10, "high");
    const path = [start, wEnd, p(3, "high"), p(4, "low"), xEnd, p(7, "low"), end];
    const c: CorrectiveCountV2 = {
      pivots: [start, wEnd, xEnd, end],
      path,
      labels: ["w", "x", "y"],
      pattern: "combination",
      checks: [],
      score: 0,
    };
    const { pts, labels } = correctiveLabelAnchors(c);
    expect(labels).toEqual(["w", "x", "y"]);
    expect(pts).toHaveLength(3);
    expect(pts.map((x) => x.index)).toEqual([1, 2, 10]);
  });

  it("uses path.slice(1) for triangle when labels match path interior count", () => {
    const pts6 = [p(0, "low"), p(1, "high"), p(2, "low"), p(3, "high"), p(4, "low"), p(5, "high")];
    const c: CorrectiveCountV2 = {
      pivots: [pts6[0]!, pts6[1]!, pts6[2]!, pts6[5]!],
      path: pts6,
      labels: ["a", "b", "c", "d", "e"],
      pattern: "triangle",
      checks: [],
      score: 0,
    };
    const { pts, labels } = correctiveLabelAnchors(c);
    expect(labels).toEqual(["a", "b", "c", "d", "e"]);
    expect(pts).toHaveLength(5);
    expect(pts.map((x) => x.index)).toEqual([1, 2, 3, 4, 5]);
  });
});
