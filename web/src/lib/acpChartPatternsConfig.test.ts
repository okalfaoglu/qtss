import { describe, expect, it } from "vitest";
import {
  ACP_PATTERN_ROWS,
  DEFAULT_ACP_CONFIG,
  acpAllowedLastPivotDirections,
  acpConfigToChannelSixOptions,
  acpEnabledPatternIds,
  acpPrimaryZigzagRow,
  filterDrawingBatchForDisplay,
  normalizeAcpChartPatternsConfig,
  patternIdsFromPatternGroups,
} from "./acpChartPatternsConfig";
import type { PatternDrawingBatchJson } from "../api/client";

describe("normalizeAcpChartPatternsConfig", () => {
  it("fills defaults for null/empty input", () => {
    const n = normalizeAcpChartPatternsConfig(null);
    expect(n.version).toBe(1);
    expect(n.ohlc).toEqual(DEFAULT_ACP_CONFIG.ohlc);
    expect(n.zigzag.length).toBe(DEFAULT_ACP_CONFIG.zigzag.length);
    expect(n.calculated_bars).toBe(DEFAULT_ACP_CONFIG.calculated_bars);
  });

  it("clamps calculated_bars and zigzag depth", () => {
    const n = normalizeAcpChartPatternsConfig({
      calculated_bars: 10,
      zigzag: [{ enabled: true, length: 5, depth: 9999 }],
    });
    expect(n.calculated_bars).toBe(50);
    expect(n.zigzag[0].depth).toBe(500);
  });

  it("coerces number_of_pivots to 5 or 6", () => {
    const five = normalizeAcpChartPatternsConfig({ scanning: { number_of_pivots: 99 } });
    expect(five.scanning.number_of_pivots).toBe(5);
    const six = normalizeAcpChartPatternsConfig({ scanning: { number_of_pivots: 6 } });
    expect(six.scanning.number_of_pivots).toBe(6);
  });

  it("preserves custom last_pivot per pattern row in custom mode", () => {
    const n = normalizeAcpChartPatternsConfig({
      scanning: { last_pivot_direction: "custom" },
      patterns: {
        "1": { enabled: true, last_pivot: "up" },
        "2": { enabled: true, last_pivot: "down" },
      },
    });
    expect(n.scanning.last_pivot_direction).toBe("custom");
    expect(n.patterns["1"].last_pivot).toBe("up");
    expect(n.patterns["2"].last_pivot).toBe("down");
  });
});

describe("patternIdsFromPatternGroups", () => {
  it("returns all 13 ids when every group flag is true", () => {
    const s = patternIdsFromPatternGroups(DEFAULT_ACP_CONFIG.pattern_groups);
    expect(s.size).toBe(13);
    for (const { id } of ACP_PATTERN_ROWS) expect(s.has(id)).toBe(true);
  });

  it("drops channel ids when channels disabled", () => {
    const g = structuredClone(DEFAULT_ACP_CONFIG.pattern_groups);
    g.geometric.channels = false;
    const s = patternIdsFromPatternGroups(g);
    expect(s.has(1)).toBe(false);
    expect(s.has(2)).toBe(false);
    expect(s.has(3)).toBe(false);
    expect(s.has(4)).toBe(true);
  });
});

describe("acpAllowedLastPivotDirections", () => {
  it("returns undefined for both mode", () => {
    const cfg = normalizeAcpChartPatternsConfig({ scanning: { last_pivot_direction: "both" } });
    expect(acpAllowedLastPivotDirections(cfg)).toBeUndefined();
  });

  it("returns direction vector for global up/down", () => {
    const up = normalizeAcpChartPatternsConfig({ scanning: { last_pivot_direction: "up" } });
    const dirs = acpAllowedLastPivotDirections(up);
    expect(dirs).toBeDefined();
    expect(dirs![1]).toBe(1);
    expect(dirs![13]).toBe(1);
    const down = normalizeAcpChartPatternsConfig({ scanning: { last_pivot_direction: "down" } });
    expect(acpAllowedLastPivotDirections(down)![5]).toBe(-1);
  });

  it("custom mode with all both yields undefined", () => {
    expect(acpAllowedLastPivotDirections(DEFAULT_ACP_CONFIG)).toBeUndefined();
  });
});

describe("acpEnabledPatternIds", () => {
  it("returns undefined when all patterns enabled and groups allow all", () => {
    expect(acpEnabledPatternIds(DEFAULT_ACP_CONFIG)).toBeUndefined();
  });

  it("returns [0] when every pattern disabled (parity guard)", () => {
    const cfg = structuredClone(DEFAULT_ACP_CONFIG);
    for (const { id } of ACP_PATTERN_ROWS) {
      cfg.patterns[String(id)].enabled = false;
    }
    expect(acpEnabledPatternIds(cfg)).toEqual([0]);
  });
});

describe("acpConfigToChannelSixOptions", () => {
  it("includes allowed_pattern_ids when filtered", () => {
    const cfg = structuredClone(DEFAULT_ACP_CONFIG);
    cfg.patterns["1"].enabled = false;
    const opts = acpConfigToChannelSixOptions(cfg, "dark");
    expect(opts.allowed_pattern_ids).toBeDefined();
    expect((opts.allowed_pattern_ids as number[]).includes(1)).toBe(false);
  });

  it("mirrors primary enabled zigzag length/depth as zigzag_length and zigzag_max_pivots", () => {
    const cfg = structuredClone(DEFAULT_ACP_CONFIG);
    cfg.zigzag[0] = { enabled: true, length: 13, depth: 34 };
    cfg.zigzag[1] = { enabled: false, length: 99, depth: 99 };
    const opts = acpConfigToChannelSixOptions(cfg, "dark");
    expect(opts.zigzag_length).toBe(13);
    expect(opts.zigzag_max_pivots).toBe(34);
    expect(acpPrimaryZigzagRow(cfg).length).toBe(13);
  });
});

describe("filterDrawingBatchForDisplay", () => {
  const batch: PatternDrawingBatchJson = {
    batch_id: "t",
    commands: [
      { kind: "zigzag_polyline", points: [], line_width: 1 },
      { kind: "pattern_label", at: { time_ms: 0, price: 0 }, text: "x" },
      { kind: "pivot_label", at: { time_ms: 0, price: 0 }, text: "p" },
    ],
  };

  it("returns undefined for undefined batch", () => {
    expect(filterDrawingBatchForDisplay(undefined, DEFAULT_ACP_CONFIG.display)).toBeUndefined();
  });

  it("filters by display toggles", () => {
    const d = { ...DEFAULT_ACP_CONFIG.display, show_zigzag: false };
    const out = filterDrawingBatchForDisplay(batch, d);
    expect(out!.commands.map((c) => c.kind)).toEqual(["pattern_label", "pivot_label"]);
  });
});
