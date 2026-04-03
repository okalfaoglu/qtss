/**
 * `app_config` anahtarı `acp_chart_patterns` — TradingView ACP [Trendoscope®] ile hizalı şema.
 */

import type { PatternDrawingBatchJson } from "../api/client";

export const ACP_CHART_PATTERNS_CONFIG_KEY = "acp_chart_patterns";

export type AcpTriState = "both" | "up" | "down";

/** Pine `lastPivotDirection`: `both`/`up`/`down` tüm desenlere uygulanır; `custom` → tablodaki tekil `last_pivot`. */
export type AcpLastPivotMode = "both" | "up" | "down" | "custom";

export type AcpOhlcSource = {
  open: string;
  high: string;
  low: string;
  close: string;
};

export type AcpZigzagRow = { enabled: boolean; length: number; depth: number };

/** Pine `abstractchartpatterns.SizeFilters` → `checkSize`. */
export type AcpSizeFilters = {
  filter_by_bar: boolean;
  min_pattern_bars: number;
  max_pattern_bars: number;
  filter_by_percent: boolean;
  min_pattern_percent: number;
  max_pattern_percent: number;
};

export type AcpScanning = {
  number_of_pivots: 5 | 6;
  error_threshold_percent: number;
  flat_threshold_percent: number;
  verify_bar_ratio: boolean;
  bar_ratio_limit: number;
  avoid_overlap: boolean;
  repaint: boolean;
  /** Pine `lastPivotDirection` (global mod; `custom` → tablo). */
  last_pivot_direction: AcpLastPivotMode;
  pivot_tail_skip_max: number;
  max_zigzag_levels: number;
  upper_direction: number;
  lower_direction: number;
  /** Pine `ScanProperties.ignoreIfEntryCrossed`. */
  ignore_if_entry_crossed: boolean;
  /** ACP Analiz: sembol / timeframe değişince (ve grafik yüklendiğinde) kanal taraması otomatik. */
  auto_scan_on_timeframe_change: boolean;
  size_filters: AcpSizeFilters;
};

export type AcpPatternRow = { enabled: boolean; last_pivot: AcpTriState };

/** Pine `allowChannels` / `allowWedges` / `allowTriangles` — geometrik grup. */
export type AcpPatternGroupsGeometric = {
  channels: boolean;
  wedges: boolean;
  triangles: boolean;
};

/** Pine `allowRisingPatterns` / `allowFallingPatterns` / `allowNonDirectionalPatterns`. */
export type AcpPatternGroupsDirection = {
  rising: boolean;
  falling: boolean;
  flat_bidirectional: boolean;
};

/** Pine `allowExpandingPatterns` / `allowContractingPatterns` / `allowParallelChannels`. */
export type AcpPatternGroupsFormationDynamics = {
  expanding: boolean;
  contracting: boolean;
  parallel: boolean;
};

export type AcpPatternGroups = {
  geometric: AcpPatternGroupsGeometric;
  direction: AcpPatternGroupsDirection;
  formation_dynamics: AcpPatternGroupsFormationDynamics;
};

export type AcpDisplay = {
  theme: "dark" | "light";
  pattern_line_width: number;
  zigzag_line_width: number;
  show_pattern_label: boolean;
  show_pivot_labels: boolean;
  show_zigzag: boolean;
  max_patterns: number;
};

export type AcpChartPatternsConfig = {
  version: 1;
  ohlc: AcpOhlcSource;
  zigzag: AcpZigzagRow[];
  scanning: AcpScanning;
  /**
   * Pine `allowedPatterns` ile aynı AND: her id için geometri + yön + dinamik bayrakları;
   * `acpEnabledPatternIds` bunu 1–13 tablo `enabled` ile kesiştirir.
   */
  pattern_groups: AcpPatternGroups;
  /** Anahtarlar "1".."13" (ACP pattern_type_id). */
  patterns: Record<string, AcpPatternRow>;
  display: AcpDisplay;
  /** TV "Calculated bars" — taramaya gönderilecek son N mum (sıralı OHLC). */
  calculated_bars: number;
};

export const ACP_PATTERN_ROWS: { id: number; label: string }[] = [
  { id: 1, label: "Ascending Channel" },
  { id: 2, label: "Descending Channel" },
  { id: 3, label: "Ranging Channel" },
  { id: 4, label: "Rising Wedge (Expanding)" },
  { id: 5, label: "Falling Wedge (Expanding)" },
  { id: 6, label: "Diverging Triangle" },
  { id: 7, label: "Ascending Triangle (Expanding)" },
  { id: 8, label: "Descending Triangle (Expanding)" },
  { id: 9, label: "Rising Wedge (Contracting)" },
  { id: 10, label: "Falling Wedge (Contracting)" },
  { id: 11, label: "Converging Triangle" },
  { id: 12, label: "Descending Triangle (Contracting)" },
  { id: 13, label: "Ascending Triangle (Contracting)" },
];

/**
 * Pine `useZigzag1..4` — TV göstergesi `zigzagLength1` / `depth1` (yalnız zigzag1 açık).
 * Daha az pivot gerekirse derinlik ACP ayarından düşürülebilir.
 */
const FALLBACK_ZIGZAG: AcpZigzagRow[] = [
  { enabled: true, length: 8, depth: 55 },
  { enabled: false, length: 13, depth: 34 },
  { enabled: false, length: 21, depth: 21 },
  { enabled: false, length: 34, depth: 13 },
];

/** Pine `lastPivotDirection == 'both'` iken `allowedLastPivotDirections` hep 0 — QTSS’te karşılığı tüm desenlerde `both`. */
function defaultPatternsRecord(): Record<string, AcpPatternRow> {
  return Object.fromEntries(
    ACP_PATTERN_ROWS.map(({ id }) => [String(id), { enabled: true, last_pivot: "both" as AcpTriState }]),
  ) as Record<string, AcpPatternRow>;
}

const DEFAULT_SIZE_FILTERS: AcpSizeFilters = {
  filter_by_bar: false,
  min_pattern_bars: 0,
  max_pattern_bars: 1000,
  filter_by_percent: false,
  min_pattern_percent: 0,
  max_pattern_percent: 100,
};

function defaultPatternGroups(): AcpPatternGroups {
  return {
    geometric: { channels: true, wedges: true, triangles: true },
    direction: { rising: true, falling: true, flat_bidirectional: true },
    formation_dynamics: { expanding: true, contracting: true, parallel: true },
  };
}

/** TV ACP göstergesi / migration 0007 varsayılanı (Pine `offset=0`, `lastPivotDirection=both`). */
export const DEFAULT_ACP_CONFIG: AcpChartPatternsConfig = {
  version: 1,
  ohlc: { open: "open", high: "high", low: "low", close: "close" },
  zigzag: FALLBACK_ZIGZAG.map((r) => ({ ...r })),
  scanning: {
    number_of_pivots: 5,
    error_threshold_percent: 20,
    flat_threshold_percent: 20,
    verify_bar_ratio: true,
    bar_ratio_limit: 0.382,
    avoid_overlap: true,
    repaint: false,
    last_pivot_direction: "both",
    pivot_tail_skip_max: 0,
    /** Pine parity: 0 = unlimited (until pivot floor breaks). */
    max_zigzag_levels: 0,
    upper_direction: 1,
    lower_direction: -1,
    ignore_if_entry_crossed: false,
    /** ACP Analiz — grafik yüklendiğinde ve sembol/TF değişince kanal taraması. */
    auto_scan_on_timeframe_change: true,
    size_filters: { ...DEFAULT_SIZE_FILTERS },
  },
  pattern_groups: defaultPatternGroups(),
  patterns: defaultPatternsRecord(),
  display: {
    theme: "dark",
    pattern_line_width: 2,
    zigzag_line_width: 1,
    show_pattern_label: true,
    show_pivot_labels: true,
    show_zigzag: true,
    max_patterns: 20,
  },
  calculated_bars: 5000,
};

function clamp(n: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, n));
}

function asRecord(x: unknown): Record<string, unknown> | null {
  return x !== null && typeof x === "object" && !Array.isArray(x) ? (x as Record<string, unknown>) : null;
}

function asBool(x: unknown, fallback: boolean): boolean {
  return typeof x === "boolean" ? x : fallback;
}

function asNum(x: unknown, fallback: number): number {
  return typeof x === "number" && Number.isFinite(x) ? x : fallback;
}

function asTri(x: unknown, fallback: AcpTriState): AcpTriState {
  return x === "up" || x === "down" || x === "both" ? x : fallback;
}

/** API / DB’den gelen JSON’u güvenli şemaya çevir. */
export function normalizeAcpChartPatternsConfig(raw: unknown): AcpChartPatternsConfig {
  const root = asRecord(raw) ?? {};
  const ohlcIn = asRecord(root.ohlc);
  const ohlc: AcpOhlcSource = {
    open: typeof ohlcIn?.open === "string" ? ohlcIn.open : DEFAULT_ACP_CONFIG.ohlc.open,
    high: typeof ohlcIn?.high === "string" ? ohlcIn.high : DEFAULT_ACP_CONFIG.ohlc.high,
    low: typeof ohlcIn?.low === "string" ? ohlcIn.low : DEFAULT_ACP_CONFIG.ohlc.low,
    close: typeof ohlcIn?.close === "string" ? ohlcIn.close : DEFAULT_ACP_CONFIG.ohlc.close,
  };

  let zigzag: AcpZigzagRow[] = DEFAULT_ACP_CONFIG.zigzag.map((r) => ({ ...r }));
  if (Array.isArray(root.zigzag)) {
    const rows = root.zigzag
      .map((z): AcpZigzagRow | null => {
        const r = asRecord(z);
        if (!r) return null;
        return {
          enabled: asBool(r.enabled, true),
          length: clamp(Math.floor(asNum(r.length, 5)), 1, 99),
          depth: clamp(Math.floor(asNum(r.depth, 32)), 1, 500),
        };
      })
      .filter((x): x is AcpZigzagRow => !!x);
    if (rows.length > 0) zigzag = rows;
  }

  const scanIn = asRecord(root.scanning);
  const sfIn = asRecord(scanIn?.size_filters);
  const size_filters: AcpSizeFilters = {
    filter_by_bar: asBool(sfIn?.filter_by_bar, DEFAULT_SIZE_FILTERS.filter_by_bar),
    min_pattern_bars: clamp(Math.floor(asNum(sfIn?.min_pattern_bars, DEFAULT_SIZE_FILTERS.min_pattern_bars)), 0, 500_000),
    max_pattern_bars: clamp(Math.floor(asNum(sfIn?.max_pattern_bars, DEFAULT_SIZE_FILTERS.max_pattern_bars)), 0, 500_000),
    filter_by_percent: asBool(sfIn?.filter_by_percent, DEFAULT_SIZE_FILTERS.filter_by_percent),
    min_pattern_percent: asNum(sfIn?.min_pattern_percent, DEFAULT_SIZE_FILTERS.min_pattern_percent),
    max_pattern_percent: asNum(sfIn?.max_pattern_percent, DEFAULT_SIZE_FILTERS.max_pattern_percent),
  };
  const scanLastPivot = scanIn?.last_pivot_direction;
  const last_pivot_direction: AcpLastPivotMode =
    scanLastPivot === "up" || scanLastPivot === "down" || scanLastPivot === "custom"
      ? scanLastPivot
      : "both";

  const scanning: AcpScanning = {
    number_of_pivots: asNum(scanIn?.number_of_pivots, 5) === 6 ? 6 : 5,
    error_threshold_percent: clamp(asNum(scanIn?.error_threshold_percent, 20), 1, 100),
    flat_threshold_percent: clamp(asNum(scanIn?.flat_threshold_percent, 20), 1, 100),
    verify_bar_ratio: asBool(scanIn?.verify_bar_ratio, true),
    bar_ratio_limit: clamp(asNum(scanIn?.bar_ratio_limit, 0.382), 0.01, 0.99),
    avoid_overlap: asBool(scanIn?.avoid_overlap, true),
    repaint: asBool(scanIn?.repaint, false),
    last_pivot_direction,
    pivot_tail_skip_max: clamp(Math.floor(asNum(scanIn?.pivot_tail_skip_max, 0)), 0, 100),
    // `0` is valid (unlimited in Rust parity mode); UI may still choose a finite value.
    max_zigzag_levels: clamp(
      Math.floor(asNum(scanIn?.max_zigzag_levels, DEFAULT_ACP_CONFIG.scanning.max_zigzag_levels)),
      0,
      8,
    ),
    upper_direction: asNum(scanIn?.upper_direction, 1),
    lower_direction: asNum(scanIn?.lower_direction, -1),
    ignore_if_entry_crossed: asBool(scanIn?.ignore_if_entry_crossed, false),
    auto_scan_on_timeframe_change: asBool(
      scanIn?.auto_scan_on_timeframe_change,
      DEFAULT_ACP_CONFIG.scanning.auto_scan_on_timeframe_change,
    ),
    size_filters,
  };

  const patternsIn = asRecord(root.patterns);
  const patterns: Record<string, AcpPatternRow> = defaultPatternsRecord();
  if (patternsIn) {
    for (const { id } of ACP_PATTERN_ROWS) {
      const k = String(id);
      const pr = asRecord(patternsIn[k]);
      if (pr) {
        patterns[k] = {
          enabled: asBool(pr.enabled, true),
          last_pivot: asTri(pr.last_pivot, patterns[k]?.last_pivot ?? "both"),
        };
      }
    }
  }

  const dispIn = asRecord(root.display);
  const display: AcpDisplay = {
    theme: dispIn?.theme === "light" ? "light" : "dark",
    pattern_line_width: clamp(Math.floor(asNum(dispIn?.pattern_line_width, 2)), 1, 8),
    zigzag_line_width: clamp(Math.floor(asNum(dispIn?.zigzag_line_width, 1)), 1, 8),
    show_pattern_label: asBool(dispIn?.show_pattern_label, true),
    show_pivot_labels: asBool(dispIn?.show_pivot_labels, true),
    show_zigzag: asBool(dispIn?.show_zigzag, true),
    max_patterns: clamp(Math.floor(asNum(dispIn?.max_patterns, 20)), 1, 32),
  };

  const calculated_bars = clamp(Math.floor(asNum(root.calculated_bars, 5000)), 50, 50_000);

  const pgIn = asRecord(root.pattern_groups);
  const geomIn = asRecord(pgIn?.geometric);
  const dirIn = asRecord(pgIn?.direction);
  const fdIn = asRecord(pgIn?.formation_dynamics);
  const defPg = defaultPatternGroups();
  const pattern_groups: AcpPatternGroups = {
    geometric: {
      channels: asBool(geomIn?.channels, defPg.geometric.channels),
      wedges: asBool(geomIn?.wedges, defPg.geometric.wedges),
      triangles: asBool(geomIn?.triangles, defPg.geometric.triangles),
    },
    direction: {
      rising: asBool(dirIn?.rising, defPg.direction.rising),
      falling: asBool(dirIn?.falling, defPg.direction.falling),
      flat_bidirectional: asBool(dirIn?.flat_bidirectional, defPg.direction.flat_bidirectional),
    },
    formation_dynamics: {
      expanding: asBool(fdIn?.expanding, defPg.formation_dynamics.expanding),
      contracting: asBool(fdIn?.contracting, defPg.formation_dynamics.contracting),
      parallel: asBool(fdIn?.parallel, defPg.formation_dynamics.parallel),
    },
  };

  return {
    version: 1,
    ohlc,
    zigzag,
    scanning,
    pattern_groups,
    patterns,
    display,
    calculated_bars,
  };
}

function triToApiDir(t: AcpTriState): number {
  if (t === "up") return 1;
  if (t === "down") return -1;
  return 0;
}

/**
 * Pine `allowedLastPivotDirections`: indeks = pattern_type_id (1..13); 0 = serbest.
 * `lastPivotDirection != 'custom'` iken tüm desenlere aynı `getLastPivotDirectionInt(lastPivotDirection)` uygulanır.
 */
export function acpAllowedLastPivotDirections(cfg: AcpChartPatternsConfig): number[] | undefined {
  const mode = cfg.scanning.last_pivot_direction ?? "both";
  const dirs = new Array<number>(14).fill(0);

  if (mode === "both") {
    return undefined;
  }
  if (mode === "up") {
    for (let id = 1; id <= 13; id++) dirs[id] = 1;
    return dirs;
  }
  if (mode === "down") {
    for (let id = 1; id <= 13; id++) dirs[id] = -1;
    return dirs;
  }

  let any = false;
  for (let id = 1; id <= 13; id++) {
    const row = cfg.patterns[String(id)];
    const t = triToApiDir(row?.last_pivot ?? "both");
    dirs[id] = t;
    if (t !== 0) any = true;
  }
  return any ? dirs : undefined;
}

/**
 * Pine `array.from(...)` ile aynı id→bayrak AND’i (kanal/takoz/üçgen × yön × expanding/contracting/parallel).
 */
export function patternIdsFromPatternGroups(g: AcpPatternGroups): Set<number> {
  const ch = g.geometric.channels;
  const wg = g.geometric.wedges;
  const tr = g.geometric.triangles;
  const rs = g.direction.rising;
  const fs = g.direction.falling;
  const nd = g.direction.flat_bidirectional;
  const ex = g.formation_dynamics.expanding;
  const co = g.formation_dynamics.contracting;
  const pa = g.formation_dynamics.parallel;

  const s = new Set<number>();
  if (ch && rs && pa) s.add(1);
  if (ch && fs && pa) s.add(2);
  if (ch && nd && pa) s.add(3);
  if (wg && rs && ex) s.add(4);
  if (wg && fs && ex) s.add(5);
  if (tr && nd && ex) s.add(6);
  if (tr && rs && ex) s.add(7);
  if (tr && fs && ex) s.add(8);
  if (wg && rs && co) s.add(9);
  if (wg && fs && co) s.add(10);
  if (tr && nd && co) s.add(11);
  if (tr && fs && co) s.add(12);
  if (tr && rs && co) s.add(13);
  return s;
}

export function acpEnabledPatternIds(cfg: AcpChartPatternsConfig): number[] | undefined {
  const fromGroups = patternIdsFromPatternGroups(cfg.pattern_groups);
  const ids = ACP_PATTERN_ROWS.map((r) => r.id).filter((id) => {
    if (cfg.patterns[String(id)]?.enabled === false) return false;
    if (!fromGroups.has(id)) return false;
    return true;
  });
  if (ids.length === 13) return undefined;
  /** Boş API’de `allowed_pattern_ids: []` → “hepsi” sayılıyor; geçersiz id ile tümünü reddettiririz. */
  if (ids.length === 0) return [0];
  return ids;
}

/** İlk `enabled` zigzag satırı; yoksa liste başı — `channel-six` yedek `zigzag_length` / `zigzag_max_pivots` ile aynı değerler. */
export function acpPrimaryZigzagRow(cfg: AcpChartPatternsConfig): AcpZigzagRow {
  const rows = cfg.zigzag;
  const on = rows.find((z) => z.enabled);
  return on ?? rows[0] ?? { enabled: true, length: 5, depth: 55 };
}

/** Kanal taraması gövdesi (bars hariç). `appTheme` grafik teması — TV display.theme yerine uygulama tercihi. */
export function acpConfigToChannelSixOptions(
  cfg: AcpChartPatternsConfig,
  appTheme: "dark" | "light",
): Record<string, unknown> {
  const s = cfg.scanning;
  const d = cfg.display;
  const allowedIds = acpEnabledPatternIds(cfg);
  const lastDirs = acpAllowedLastPivotDirections(cfg);
  const primary = acpPrimaryZigzagRow(cfg);
  return {
    zigzag_configs: cfg.zigzag.map((z) => ({
      enabled: z.enabled,
      length: z.length,
      depth: z.depth,
    })),
    /** API yedek yolu (`zigzag_configs` içinde hiç `enabled` yoksa) — seçili satırın length/depth ile hizalı olsun. */
    zigzag_length: primary.length,
    zigzag_max_pivots: primary.depth,
    number_of_pivots: s.number_of_pivots,
    bar_ratio_enabled: s.verify_bar_ratio,
    bar_ratio_limit: s.bar_ratio_limit,
    flat_ratio: s.flat_threshold_percent / 100,
    error_score_ratio_max: s.error_threshold_percent / 100,
    upper_direction: s.upper_direction,
    lower_direction: s.lower_direction,
    pivot_tail_skip_max: s.pivot_tail_skip_max,
    max_zigzag_levels: s.max_zigzag_levels,
    avoid_overlap: s.avoid_overlap,
    repaint: s.repaint,
    ignore_if_entry_crossed: s.ignore_if_entry_crossed,
    size_filters: s.size_filters,
    max_matches: d.max_patterns,
    theme_dark: appTheme === "dark",
    pattern_line_width: d.pattern_line_width,
    zigzag_line_width: d.zigzag_line_width,
    ...(allowedIds !== undefined ? { allowed_pattern_ids: allowedIds } : {}),
    ...(lastDirs ? { allowed_last_pivot_directions: lastDirs } : {}),
  };
}

export function filterDrawingBatchForDisplay(
  batch: PatternDrawingBatchJson | undefined,
  display: AcpDisplay,
): PatternDrawingBatchJson | undefined {
  if (!batch) return undefined;
  const cmds = batch.commands.filter((c) => {
    if (c.kind === "zigzag_polyline" && !display.show_zigzag) return false;
    if (c.kind === "pattern_label" && !display.show_pattern_label) return false;
    if (c.kind === "pivot_label" && !display.show_pivot_labels) return false;
    return true;
  });
  return { ...batch, commands: cmds };
}

export const ACP_OHLC_PRESETS = ["open", "high", "low", "close"] as const;
