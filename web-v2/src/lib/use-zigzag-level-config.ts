// Multi-level ZigZag render config hook.
//
// Reads `chart.zigzag.l{0,1,2,3}.{color,width,style,enabled}` from
// system_config (migration 0189) and exposes a typed view the Chart
// component consumes. DB is the single source of truth (CLAUDE.md #2);
// defaults below only cover the pre-fetch bootstrap window so the chart
// never renders un-styled.

import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "./api";
import type { ConfigEditorView } from "./types";

export type PivotLevel = "L0" | "L1" | "L2" | "L3";
export type ZigzagLineStyle = "solid" | "dashed" | "dotted";

export interface ZigzagLevelStyle {
  color: string;
  width: number;
  style: ZigzagLineStyle;
  enabledByDefault: boolean;
}

export interface ZigzagLevelConfig {
  levels: Record<PivotLevel, ZigzagLevelStyle>;
  maxPointsPerLevel: number;
}

const DEFAULTS: Record<PivotLevel, ZigzagLevelStyle> = {
  L0: { color: "#94a3b8", width: 1, style: "dotted", enabledByDefault: false },
  L1: { color: "#42a5f5", width: 2, style: "solid",  enabledByDefault: true  },
  L2: { color: "#ab47bc", width: 2, style: "solid",  enabledByDefault: false },
  L3: { color: "#ff7043", width: 3, style: "solid",  enabledByDefault: false },
};

const KEY_PREFIX = "chart.zigzag.";
const LEVELS: PivotLevel[] = ["L0", "L1", "L2", "L3"];

function parseHex(raw: unknown): string | null {
  if (typeof raw !== "string") return null;
  const t = raw.trim();
  return /^#[0-9a-f]{6}$/i.test(t) ? t : null;
}
function parseStyle(raw: unknown): ZigzagLineStyle | null {
  return raw === "solid" || raw === "dashed" || raw === "dotted" ? raw : null;
}
function parseInt1(raw: unknown): number | null {
  const n = typeof raw === "number" ? raw : Number(raw);
  return Number.isFinite(n) && n >= 1 && n <= 8 ? Math.round(n) : null;
}
function parseBool(raw: unknown): boolean | null {
  return typeof raw === "boolean" ? raw : null;
}

function build(view: ConfigEditorView | undefined): ZigzagLevelConfig {
  const out: Record<PivotLevel, ZigzagLevelStyle> = {
    L0: { ...DEFAULTS.L0 },
    L1: { ...DEFAULTS.L1 },
    L2: { ...DEFAULTS.L2 },
    L3: { ...DEFAULTS.L3 },
  };
  let maxPoints = 2_000;
  const chart = view?.groups.find((g) => g.module === "chart");
  for (const entry of chart?.entries ?? []) {
    if (!entry.config_key.startsWith(KEY_PREFIX)) continue;
    const suffix = entry.config_key.slice(KEY_PREFIX.length);
    if (suffix === "max_points_per_level") {
      const n = Number(entry.value);
      if (Number.isFinite(n) && n > 0) maxPoints = Math.round(n);
      continue;
    }
    const [lvlRaw, field] = suffix.split(".");
    const lvl = lvlRaw?.toUpperCase() as PivotLevel | undefined;
    if (!lvl || !LEVELS.includes(lvl)) continue;
    const target = out[lvl];
    const setters: Record<string, () => void> = {
      color:   () => { const v = parseHex(entry.value);   if (v) target.color = v; },
      width:   () => { const v = parseInt1(entry.value);  if (v) target.width = v; },
      style:   () => { const v = parseStyle(entry.value); if (v) target.style = v; },
      enabled: () => { const v = parseBool(entry.value);  if (v !== null) target.enabledByDefault = v; },
    };
    setters[field]?.();
  }
  return { levels: out, maxPointsPerLevel: maxPoints };
}

export function useZigzagLevelConfig(): ZigzagLevelConfig {
  const { data } = useQuery({
    queryKey: ["chart-zigzag-levels"],
    queryFn: () => apiFetch<ConfigEditorView>("/v2/config?limit=500"),
    staleTime: 60_000,
    refetchOnMount: false,
    refetchOnWindowFocus: false,
  });
  return build(data);
}
