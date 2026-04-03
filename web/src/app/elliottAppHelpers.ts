import type { PatternLayerOverlay } from "../lib/patternDrawingBatchOverlay";
import {
  mergePatternMenuOrTf,
  type ElliottAnalysisTimeframe,
  type ElliottWaveConfig,
} from "../lib/elliottWaveAppConfig";
import type { ElliottPatternMenuToggles } from "../lib/elliottPatternMenuCatalog";

/** V2 raw ZigZag overlay kinds — matches adapter `zigzagKind`. */
export function keepElliottZigzagLayer(
  kind: PatternLayerOverlay["zigzagKind"],
  c: Pick<
    ElliottWaveConfig,
    | "show_zigzag_pivot_1w"
    | "show_zigzag_pivot_1d"
    | "show_zigzag_pivot_4h"
    | "show_zigzag_pivot_1h"
    | "show_zigzag_pivot_15m"
  >,
): boolean {
  if (kind === "elliott_v2_zigzag_weekly") return c.show_zigzag_pivot_1w;
  if (kind === "elliott_v2_zigzag_daily") return c.show_zigzag_pivot_1d;
  if (kind === "elliott_v2_zigzag_macro") return c.show_zigzag_pivot_4h;
  if (kind === "elliott_v2_zigzag_intermediate") return c.show_zigzag_pivot_1h;
  if (kind === "elliott_v2_zigzag_micro") return c.show_zigzag_pivot_15m;
  return true;
}

/** V2 raw ZigZag polyline (not impulse / corrective drawing layers). */
export function isV2RawZigzagKind(kind: PatternLayerOverlay["zigzagKind"] | undefined): boolean {
  return (
    kind === "elliott_v2_zigzag_weekly" ||
    kind === "elliott_v2_zigzag_daily" ||
    kind === "elliott_v2_zigzag_macro" ||
    kind === "elliott_v2_zigzag_intermediate" ||
    kind === "elliott_v2_zigzag_micro"
  );
}

export function patchPatternMenuTf(
  c: ElliottWaveConfig,
  tf: ElliottAnalysisTimeframe,
  key: keyof ElliottPatternMenuToggles,
  checked: boolean,
): ElliottWaveConfig {
  const pattern_menu_by_tf = {
    ...c.pattern_menu_by_tf,
    [tf]: { ...c.pattern_menu_by_tf[tf], [key]: checked },
  };
  const pattern_menu = mergePatternMenuOrTf(pattern_menu_by_tf);
  return {
    ...c,
    pattern_menu_by_tf,
    pattern_menu,
    formations: { ...c.formations, impulse: pattern_menu.motive_impulse },
  };
}

/** `<input type="color" />` — normalize #RGB → #RRGGBB */
export function elliottColorInputValue(hex: string): string {
  const t = hex.trim();
  if (/^#[0-9A-Fa-f]{6}$/i.test(t)) return t.toLowerCase();
  if (/^#[0-9A-Fa-f]{3}$/i.test(t)) {
    const a = t.slice(1);
    const r = a[0]!;
    const g = a[1]!;
    const b = a[2]!;
    return `#${r}${r}${g}${g}${b}${b}`.toLowerCase();
  }
  return "#e53935";
}
