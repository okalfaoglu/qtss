import type { PatternLayerOverlay } from "../lib/patternDrawingBatchOverlay";
import {
  elliottDrawingConfigKeys,
  elliottProjectionConfigKey,
  mergePatternMenuOrTf,
  type ElliottAnalysisTimeframe,
  type ElliottWaveConfig,
} from "../lib/elliottWaveAppConfig";
import {
  DEFAULT_ELLIOTT_PATTERN_MENU,
  ELLIOTT_PATTERN_MENU_ROWS,
  type ElliottPatternMenuToggles,
} from "../lib/elliottPatternMenuCatalog";

const ELLIOTT_PATTERN_MENU_LEAF_KEYS = ELLIOTT_PATTERN_MENU_ROWS.filter(
  (row): row is Extract<(typeof ELLIOTT_PATTERN_MENU_ROWS)[number], { type: "toggle" }> => row.type === "toggle",
).map((row) => row.id);

function patternMenuAllDisabled(): ElliottPatternMenuToggles {
  return {
    motive_impulse: false,
    motive_diagonal_leading: false,
    motive_diagonal_ending: false,
    corrective_zigzag: false,
    corrective_flat: false,
    corrective_complex_double: false,
    corrective_complex_triple: false,
    corrective_triangle: false,
  };
}

/** Master column: pattern menu + projeksiyon + dalga/ZigZag görünürlüğü (renk/çizgi tipi değişmez). */
export function elliottAnalysisTimeframeColumnCheckboxState(
  c: ElliottWaveConfig,
  tf: ElliottAnalysisTimeframe,
): { checked: boolean; indeterminate: boolean } {
  const m = c.pattern_menu_by_tf[tf];
  const patternOn = ELLIOTT_PATTERN_MENU_LEAF_KEYS.filter((k) => m[k]).length;
  const dk = elliottDrawingConfigKeys(tf);
  const drawOn =
    (c[dk.showLine] ? 1 : 0) + (c[dk.showLabel] ? 1 : 0) + (c[dk.showZigzagPivot] ? 1 : 0);
  const proj = c[elliottProjectionConfigKey(tf)] ? 1 : 0;
  const total = ELLIOTT_PATTERN_MENU_LEAF_KEYS.length + 3 + 1;
  const on = patternOn + drawOn + proj;
  if (on === 0) return { checked: false, indeterminate: false };
  if (on === total) return { checked: true, indeterminate: false };
  return { checked: false, indeterminate: true };
}

export function setElliottAnalysisTimeframeColumnEnabled(
  c: ElliottWaveConfig,
  tf: ElliottAnalysisTimeframe,
  enabled: boolean,
): ElliottWaveConfig {
  const dk = elliottDrawingConfigKeys(tf);
  const pk = elliottProjectionConfigKey(tf);
  const pattern_menu_by_tf = {
    ...c.pattern_menu_by_tf,
    [tf]: enabled ? { ...DEFAULT_ELLIOTT_PATTERN_MENU } : patternMenuAllDisabled(),
  };
  const pattern_menu = mergePatternMenuOrTf(pattern_menu_by_tf);
  return {
    ...c,
    pattern_menu_by_tf,
    pattern_menu,
    formations: { ...c.formations, impulse: pattern_menu.motive_impulse },
    [pk]: enabled,
    [dk.showLine]: enabled,
    [dk.showLabel]: enabled,
    [dk.showZigzagPivot]: enabled,
  } as ElliottWaveConfig;
}

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
