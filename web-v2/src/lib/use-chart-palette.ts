// Aşama 5.C — overlay renk paleti hook'u.
//
// `ui.chart.palette.family.*` ve `ui.chart.palette.style.*` anahtarlarını
// DB'den çeker (Config Editor'dan canlı değiştirilebilir — CLAUDE.md #2).
// Bootstrap sırasında ağ dönmeden önce hardcoded DEFAULTS kullanılır;
// fetch tamamlandığında DB override'ları DEFAULT'lara yazılır.
//
// Chart.tsx kullanım:
//   const palette = useChartPalette();
//   const color = palette.colorFor(d.family, d.render_style);

import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "./api";
import type { ConfigEditorView } from "./types";

export interface ChartPalette {
  family: Record<string, string>;
  style: Record<string, string>;
  colorFor(family: string, styleKey?: string | null): string;
}

const DEFAULTS_FAMILY: Record<string, string> = {
  elliott: "#7dd3fc",
  harmonic: "#f472b6",
  classical: "#facc15",
  wyckoff: "#a78bfa",
  range: "#5eead4",
  tbm: "#fb923c",
  candle: "#fca5a5",
  gap: "#38bdf8",
  custom: "#d4d4d8",
};

const DEFAULTS_STYLE: Record<string, string> = {
  bull: "#22c55e",
  bear: "#ef4444",
};

const FAMILY_PREFIX = "chart.palette.family.";
const STYLE_PREFIX = "chart.palette.style.";

function extractHex(raw: unknown): string | null {
  if (typeof raw !== "string") return null;
  const trimmed = raw.trim();
  return /^#[0-9a-f]{6}$/i.test(trimmed) ? trimmed : null;
}

function buildPalette(view: ConfigEditorView | undefined): ChartPalette {
  const family: Record<string, string> = { ...DEFAULTS_FAMILY };
  const style: Record<string, string> = { ...DEFAULTS_STYLE };
  const ui = view?.groups.find(g => g.module === "ui");
  for (const entry of ui?.entries ?? []) {
    const hex = extractHex(entry.value);
    if (!hex) continue;
    if (entry.config_key.startsWith(FAMILY_PREFIX)) {
      family[entry.config_key.slice(FAMILY_PREFIX.length)] = hex;
    } else if (entry.config_key.startsWith(STYLE_PREFIX)) {
      style[entry.config_key.slice(STYLE_PREFIX.length)] = hex;
    }
  }
  return {
    family,
    style,
    colorFor(fam, key) {
      if (key && style[key]) return style[key];
      return family[fam] ?? family.custom;
    },
  };
}

export function useChartPalette(): ChartPalette {
  const { data } = useQuery({
    queryKey: ["chart-palette"],
    queryFn: () => apiFetch<ConfigEditorView>("/v2/config?limit=500"),
    staleTime: 60_000,
    // Palette is latency-insensitive — keep stale data between mounts
    // so the chart never flashes back to defaults on navigation.
    refetchOnMount: false,
    refetchOnWindowFocus: false,
  });
  return buildPalette(data);
}
