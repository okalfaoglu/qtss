/**
 * `app_config` anahtarı `elliott_wave` — web panel + GET `/analysis/elliott-wave-config`.
 */

import {
  DEFAULT_ELLIOTT_PATTERN_MENU,
  type ElliottPatternMenuToggles,
} from "./elliottPatternMenuCatalog";

export type { ElliottPatternMenuToggles };

/** Dalga türü anahtarları — her TF için ayrı açılıp kapatılır (motor + çizim). */
export type ElliottPatternMenuByTf = {
  "4h": ElliottPatternMenuToggles;
  "1h": ElliottPatternMenuToggles;
  "15m": ElliottPatternMenuToggles;
};

export function defaultPatternMenuByTf(base?: ElliottPatternMenuToggles): ElliottPatternMenuByTf {
  const m = { ...DEFAULT_ELLIOTT_PATTERN_MENU, ...base };
  return { "4h": { ...m }, "1h": { ...m }, "15m": { ...m } };
}

/** Geriye dönük: tek `pattern_menu` alanını «herhangi bir TF açık mı?» olarak birleştirir. */
export function mergePatternMenuOrTf(m: ElliottPatternMenuByTf): ElliottPatternMenuToggles {
  return {
    motive_impulse:
      m["4h"].motive_impulse || m["1h"].motive_impulse || m["15m"].motive_impulse,
    motive_diagonal:
      m["4h"].motive_diagonal || m["1h"].motive_diagonal || m["15m"].motive_diagonal,
    corrective_zigzag:
      m["4h"].corrective_zigzag || m["1h"].corrective_zigzag || m["15m"].corrective_zigzag,
    corrective_flat:
      m["4h"].corrective_flat || m["1h"].corrective_flat || m["15m"].corrective_flat,
    corrective_triangle:
      m["4h"].corrective_triangle || m["1h"].corrective_triangle || m["15m"].corrective_triangle,
    corrective_complex_wxy:
      m["4h"].corrective_complex_wxy ||
      m["1h"].corrective_complex_wxy ||
      m["15m"].corrective_complex_wxy,
  };
}

export function patternMenuForTf(c: ElliottWaveConfig, tf: keyof ElliottPatternMenuByTf): ElliottPatternMenuToggles {
  return { ...DEFAULT_ELLIOTT_PATTERN_MENU, ...c.pattern_menu_by_tf[tf] };
}

export const ELLIOTT_WAVE_CONFIG_KEY = "elliott_wave";

/** Grafik: 4h / 1h / 15m Elliott çizgileri (menüden özelleştirilebilir). */
export type ElliottMtfWaveColors = {
  "4h": string;
  "1h": string;
  "15m": string;
};

export const DEFAULT_ELLIOTT_MTF_WAVE_COLORS: ElliottMtfWaveColors = {
  "4h": "#e53935",
  "1h": "#43a047",
  "15m": "#fb8c00",
};

/** Geçerli #RGB veya #RRGGBB; aksi halde fallback. */
export function sanitizeElliottHexColor(raw: unknown, fallback: string): string {
  if (typeof raw !== "string") return fallback;
  const t = raw.trim();
  if (/^#[0-9A-Fa-f]{6}$/.test(t)) return t;
  if (/^#[0-9A-Fa-f]{3}$/.test(t)) return t;
  return fallback;
}

/** Projeksiyon ikinci senaryo çizgisi: #RRGGBB kanallarını `factor` ile çarpar (0–1, örn. 0.62). */
export function scaleElliottHexColor(hex: string, factor: number): string {
  const t = hex.trim();
  const m6 = /^#([0-9A-Fa-f]{6})$/i.exec(t);
  const m3 = /^#([0-9A-Fa-f]{3})$/i.exec(t);
  const f = clamp(factor, 0.15, 1);
  const push = (r: number, g: number, b: number) =>
    `#${Math.min(255, Math.round(r * f))
      .toString(16)
      .padStart(2, "0")}${Math.min(255, Math.round(g * f))
      .toString(16)
      .padStart(2, "0")}${Math.min(255, Math.round(b * f)).toString(16).padStart(2, "0")}`;
  if (m6) {
    const h = m6[1];
    return push(parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16));
  }
  if (m3) {
    const h = m3[1];
    return push(
      parseInt(h[0] + h[0], 16),
      parseInt(h[1] + h[1], 16),
      parseInt(h[2] + h[2], 16),
    );
  }
  return t;
}

function clamp(n: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, n));
}

export type ElliottWaveFormations = {
  /** 1–5 itki çizgisi ve dalga numaraları */
  impulse: boolean;
};

export type ElliottWaveConfig = {
  version: 1;
  /** Analiz ve grafik katmanı için ana anahtar */
  enabled: boolean;
  formations: ElliottWaveFormations;
  /**
   * Pine “Elliott Wave Predictor” tarzı ileri Fib projeksiyon çizgisi (şema; tavsiye değil).
   * Ana itki açıkken ve analiz varsa son mumdan itibaren çizilir.
   */
  /** İleri Fib şeması — TF başına (ilgili itkiden projekte eder). */
  show_projection_4h: boolean;
  show_projection_1h: boolean;
  show_projection_15m: boolean;
  /** Geçmiş verilerde Elliott itki yapıları ara ve grafikte göster (ince katman). */
  show_historical_waves: boolean;
  /** Ana itkı içindeki alt itkı (1/3/5) ve dalga 2/4 içi mikro a–b–c çizimleri. */
  show_nested_formations: boolean;
  /** Projeksiyonda her adım = kaç mum süresi (Pine varsayılan 22). */
  projection_bar_hop: number;
  /** Kaç segment ileri (Pine 12; üst sınır 24). */
  projection_steps: number;
  /**
   * İleri projeksiyonda ikinci çizgi: daha uzun 3. dalga hedefi (kalibre çarpanı yükseltilmiş şema).
   * Kapalıysa yalnızca birincil polyline çizilir.
   */
  show_projection_alt_scenario: boolean;
  /**
   * @deprecated ACP zigzag kanal/tarama ayarıdır; Elliott V2 ZigZag artık bunu kullanmaz.
   * Geriye dönük JSON uyumluluğu için saklanır.
   */
  use_acp_zigzag_swing: boolean;
  /** @deprecated ACP zigzag satırı; Elliott motoru ile ilişkili değildir. */
  acp_zigzag_row_index: number;
  /**
   * Elliott V2 ZigZag (fraktal pencere): her iki yanda kaç mum (TradingView ZigZag ile uyumlu).
   * ACP `zigzag[]` değişkeninden bağımsızdır. Geriye dönük: tek değer; yeni kayıtlar TF başına alanlarla senkron tutulur.
   */
  elliott_zigzag_depth: number;
  /** Elliott V2 ZigZag depth — 4H MTF. */
  elliott_zigzag_depth_4h: number;
  /** Elliott V2 ZigZag depth — 1H MTF. */
  elliott_zigzag_depth_1h: number;
  /** Elliott V2 ZigZag depth — 15M MTF. */
  elliott_zigzag_depth_15m: number;
  /** @deprecated Eski alan; normalize `elliott_zigzag_depth` ile doldurulur. */
  swing_depth: number;
  max_pivot_windows: number;
  strict_wave4_overlap: boolean;
  /**
   * Menüdeki dalga türleri — düzeltme motoru hangi kalıpları deneyeceğini filtreler (varsayılan hepsi açık).
   * @deprecated Yeni kayıtlar `pattern_menu_by_tf` kullanır; normalize sırasında OR ile doldurulur.
   */
  pattern_menu: ElliottPatternMenuToggles;
  /** Dalga türleri — TF başına (motor + hangi çizimlerin üretileceği). */
  pattern_menu_by_tf: ElliottPatternMenuByTf;
  /** MTF ZigZag + dalga çizgileri rengi (hex). */
  mtf_wave_color_4h: string;
  mtf_wave_color_1h: string;
  mtf_wave_color_15m: string;
  /** MTF etiket rengi (hex). */
  mtf_label_color_4h: string;
  mtf_label_color_1h: string;
  mtf_label_color_15m: string;
  /** MTF çizgi görünürlüğü. */
  show_line_4h: boolean;
  show_line_1h: boolean;
  show_line_15m: boolean;
  /** MTF etiket görünürlüğü. */
  show_label_4h: boolean;
  show_label_1h: boolean;
  show_label_15m: boolean;
  /** MTF çizgi tipi. */
  mtf_line_style_4h: "solid" | "dotted" | "dashed";
  mtf_line_style_1h: "solid" | "dotted" | "dashed";
  mtf_line_style_15m: "solid" | "dotted" | "dashed";
  /** MTF çizgi kalınlığı. */
  mtf_line_width_4h: number;
  mtf_line_width_1h: number;
  mtf_line_width_15m: number;
  /** Ham ZigZag pivot çizgisi — TF başına görünürlük (DB). */
  show_zigzag_pivot_4h: boolean;
  show_zigzag_pivot_1h: boolean;
  show_zigzag_pivot_15m: boolean;
  /** Ham ZigZag çizgi rengi (dalga çizgilerinden bağımsız). */
  mtf_zigzag_color_4h: string;
  mtf_zigzag_color_1h: string;
  mtf_zigzag_color_15m: string;
  mtf_zigzag_line_style_4h: "solid" | "dotted" | "dashed";
  mtf_zigzag_line_style_1h: "solid" | "dotted" | "dashed";
  mtf_zigzag_line_style_15m: "solid" | "dotted" | "dashed";
  mtf_zigzag_line_width_4h: number;
  mtf_zigzag_line_width_1h: number;
  mtf_zigzag_line_width_15m: number;
};

export function mtfWaveColorsFromConfig(
  c: Pick<ElliottWaveConfig, "mtf_wave_color_4h" | "mtf_wave_color_1h" | "mtf_wave_color_15m">,
): ElliottMtfWaveColors {
  return {
    "4h": sanitizeElliottHexColor(c.mtf_wave_color_4h, DEFAULT_ELLIOTT_MTF_WAVE_COLORS["4h"]),
    "1h": sanitizeElliottHexColor(c.mtf_wave_color_1h, DEFAULT_ELLIOTT_MTF_WAVE_COLORS["1h"]),
    "15m": sanitizeElliottHexColor(c.mtf_wave_color_15m, DEFAULT_ELLIOTT_MTF_WAVE_COLORS["15m"]),
  };
}

export function mtfZigzagColorsFromConfig(
  c: Pick<
    ElliottWaveConfig,
    "mtf_zigzag_color_4h" | "mtf_zigzag_color_1h" | "mtf_zigzag_color_15m"
  >,
): ElliottMtfWaveColors {
  return {
    "4h": sanitizeElliottHexColor(c.mtf_zigzag_color_4h, DEFAULT_ELLIOTT_MTF_WAVE_COLORS["4h"]),
    "1h": sanitizeElliottHexColor(c.mtf_zigzag_color_1h, DEFAULT_ELLIOTT_MTF_WAVE_COLORS["1h"]),
    "15m": sanitizeElliottHexColor(c.mtf_zigzag_color_15m, DEFAULT_ELLIOTT_MTF_WAVE_COLORS["15m"]),
  };
}

export const DEFAULT_ELLIOTT_WAVE_CONFIG: ElliottWaveConfig = {
  version: 1,
  enabled: false,
  formations: {
    impulse: true,
  },
  show_projection_4h: false,
  show_projection_1h: false,
  show_projection_15m: false,
  show_historical_waves: false,
  show_nested_formations: true,
  projection_bar_hop: 22,
  projection_steps: 12,
  show_projection_alt_scenario: true,
  use_acp_zigzag_swing: false,
  acp_zigzag_row_index: 0,
  elliott_zigzag_depth: 21,
  elliott_zigzag_depth_4h: 21,
  elliott_zigzag_depth_1h: 21,
  elliott_zigzag_depth_15m: 21,
  swing_depth: 21,
  max_pivot_windows: 120,
  strict_wave4_overlap: false,
  pattern_menu: { ...DEFAULT_ELLIOTT_PATTERN_MENU },
  pattern_menu_by_tf: defaultPatternMenuByTf(),
  mtf_wave_color_4h: DEFAULT_ELLIOTT_MTF_WAVE_COLORS["4h"],
  mtf_wave_color_1h: DEFAULT_ELLIOTT_MTF_WAVE_COLORS["1h"],
  mtf_wave_color_15m: DEFAULT_ELLIOTT_MTF_WAVE_COLORS["15m"],
  mtf_label_color_4h: DEFAULT_ELLIOTT_MTF_WAVE_COLORS["4h"],
  mtf_label_color_1h: DEFAULT_ELLIOTT_MTF_WAVE_COLORS["1h"],
  mtf_label_color_15m: DEFAULT_ELLIOTT_MTF_WAVE_COLORS["15m"],
  show_line_4h: true,
  show_line_1h: true,
  show_line_15m: true,
  show_label_4h: true,
  show_label_1h: true,
  show_label_15m: true,
  mtf_line_style_4h: "solid",
  mtf_line_style_1h: "dashed",
  mtf_line_style_15m: "dotted",
  mtf_line_width_4h: 4,
  mtf_line_width_1h: 3,
  mtf_line_width_15m: 2,
  show_zigzag_pivot_4h: true,
  show_zigzag_pivot_1h: true,
  show_zigzag_pivot_15m: true,
  mtf_zigzag_color_4h: DEFAULT_ELLIOTT_MTF_WAVE_COLORS["4h"],
  mtf_zigzag_color_1h: DEFAULT_ELLIOTT_MTF_WAVE_COLORS["1h"],
  mtf_zigzag_color_15m: DEFAULT_ELLIOTT_MTF_WAVE_COLORS["15m"],
  mtf_zigzag_line_style_4h: "dotted",
  mtf_zigzag_line_style_1h: "dotted",
  mtf_zigzag_line_style_15m: "dotted",
  mtf_zigzag_line_width_4h: 2,
  mtf_zigzag_line_width_1h: 2,
  mtf_zigzag_line_width_15m: 2,
};

function isRecord(x: unknown): x is Record<string, unknown> {
  return x !== null && typeof x === "object" && !Array.isArray(x);
}

export function normalizeElliottWaveConfig(raw: unknown): ElliottWaveConfig {
  const base = {
    ...DEFAULT_ELLIOTT_WAVE_CONFIG,
    formations: { ...DEFAULT_ELLIOTT_WAVE_CONFIG.formations },
    pattern_menu: { ...DEFAULT_ELLIOTT_PATTERN_MENU },
    pattern_menu_by_tf: defaultPatternMenuByTf(),
  };
  if (!isRecord(raw)) return base;

  const enabled = typeof raw.enabled === "boolean" ? raw.enabled : base.enabled;

  let formations = { ...base.formations };
  if (isRecord(raw.formations)) {
    if (typeof raw.formations.impulse === "boolean") formations.impulse = raw.formations.impulse;
  }

  const ELLIOTT_ZZ_MIN = 2;
  const ELLIOTT_ZZ_MAX = 100;

  let elliott_zigzag_depth = base.elliott_zigzag_depth;
  if (typeof raw.elliott_zigzag_depth === "number" && Number.isFinite(raw.elliott_zigzag_depth)) {
    elliott_zigzag_depth = Math.min(ELLIOTT_ZZ_MAX, Math.max(ELLIOTT_ZZ_MIN, Math.floor(raw.elliott_zigzag_depth)));
  } else if (typeof raw.swing_depth === "number" && Number.isFinite(raw.swing_depth)) {
    /* Eski kayıtlar: yalnızca swing_depth vardı (1–12). */
    elliott_zigzag_depth = Math.min(ELLIOTT_ZZ_MAX, Math.max(ELLIOTT_ZZ_MIN, Math.floor(raw.swing_depth)));
  }

  const swing_depth = elliott_zigzag_depth;

  const zzDepth = (k: string) => {
    const v = raw[k];
    if (typeof v === "number" && Number.isFinite(v)) {
      return Math.min(ELLIOTT_ZZ_MAX, Math.max(ELLIOTT_ZZ_MIN, Math.floor(v)));
    }
    return elliott_zigzag_depth;
  };
  const elliott_zigzag_depth_4h = zzDepth("elliott_zigzag_depth_4h");
  const elliott_zigzag_depth_1h = zzDepth("elliott_zigzag_depth_1h");
  const elliott_zigzag_depth_15m = zzDepth("elliott_zigzag_depth_15m");

  const max_pivot_windows =
    typeof raw.max_pivot_windows === "number" && Number.isFinite(raw.max_pivot_windows)
      ? Math.min(400, Math.max(5, Math.floor(raw.max_pivot_windows)))
      : base.max_pivot_windows;

  const strict_wave4_overlap =
    typeof raw.strict_wave4_overlap === "boolean" ? raw.strict_wave4_overlap : base.strict_wave4_overlap;

  const legacyShowProj =
    typeof raw.show_projection === "boolean" ? raw.show_projection : undefined;
  const projTri = (key: "show_projection_4h" | "show_projection_1h" | "show_projection_15m") => {
    const v = raw[key];
    if (typeof v === "boolean") return v;
    if (legacyShowProj === true) return true;
    if (legacyShowProj === false) return false;
    return base[key];
  };
  const show_projection_4h = projTri("show_projection_4h");
  const show_projection_1h = projTri("show_projection_1h");
  const show_projection_15m = projTri("show_projection_15m");
  const show_historical_waves =
    typeof raw.show_historical_waves === "boolean" ? raw.show_historical_waves : base.show_historical_waves;
  const show_nested_formations =
    typeof raw.show_nested_formations === "boolean" ? raw.show_nested_formations : base.show_nested_formations;

  const projection_bar_hop =
    typeof raw.projection_bar_hop === "number" && Number.isFinite(raw.projection_bar_hop)
      ? Math.min(100, Math.max(1, Math.floor(raw.projection_bar_hop)))
      : base.projection_bar_hop;

  const projection_steps =
    typeof raw.projection_steps === "number" && Number.isFinite(raw.projection_steps)
      ? Math.min(24, Math.max(1, Math.floor(raw.projection_steps)))
      : base.projection_steps;

  const show_projection_alt_scenario =
    typeof raw.show_projection_alt_scenario === "boolean"
      ? raw.show_projection_alt_scenario
      : base.show_projection_alt_scenario;

  const use_acp_zigzag_swing =
    typeof raw.use_acp_zigzag_swing === "boolean" ? raw.use_acp_zigzag_swing : base.use_acp_zigzag_swing;

  const acp_zigzag_row_index =
    typeof raw.acp_zigzag_row_index === "number" && Number.isFinite(raw.acp_zigzag_row_index)
      ? Math.min(3, Math.max(0, Math.floor(raw.acp_zigzag_row_index)))
      : base.acp_zigzag_row_index;

  let pattern_menu = { ...base.pattern_menu };
  const pmRaw = raw.pattern_menu;
  if (isRecord(pmRaw)) {
    const b = (k: keyof ElliottPatternMenuToggles) =>
      typeof pmRaw[k] === "boolean" ? pmRaw[k] : pattern_menu[k];
    pattern_menu = {
      motive_impulse: b("motive_impulse"),
      motive_diagonal: b("motive_diagonal"),
      corrective_zigzag: b("corrective_zigzag"),
      corrective_flat: b("corrective_flat"),
      corrective_triangle: b("corrective_triangle"),
      corrective_complex_wxy: b("corrective_complex_wxy"),
    };
  }

  let pattern_menu_by_tf = defaultPatternMenuByTf(pattern_menu);
  const pmTfRaw = raw.pattern_menu_by_tf;
  if (isRecord(pmTfRaw)) {
    const tfPatch = (tf: keyof ElliottPatternMenuByTf) => {
      const o = pmTfRaw[tf];
      if (!isRecord(o)) return pattern_menu_by_tf[tf];
      const z = (k: keyof ElliottPatternMenuToggles) =>
        typeof o[k] === "boolean" ? o[k] : pattern_menu_by_tf[tf][k];
      return {
        motive_impulse: z("motive_impulse"),
        motive_diagonal: z("motive_diagonal"),
        corrective_zigzag: z("corrective_zigzag"),
        corrective_flat: z("corrective_flat"),
        corrective_triangle: z("corrective_triangle"),
        corrective_complex_wxy: z("corrective_complex_wxy"),
      };
    };
    pattern_menu_by_tf = {
      "4h": tfPatch("4h"),
      "1h": tfPatch("1h"),
      "15m": tfPatch("15m"),
    };
  }

  pattern_menu = mergePatternMenuOrTf(pattern_menu_by_tf);
  // UI: ayrı "formations.impulse" — herhangi bir TF'de itki açıksa true.
  formations.impulse = pattern_menu.motive_impulse;

  const mtf_wave_color_4h = sanitizeElliottHexColor(raw.mtf_wave_color_4h, base.mtf_wave_color_4h);
  const mtf_wave_color_1h = sanitizeElliottHexColor(raw.mtf_wave_color_1h, base.mtf_wave_color_1h);
  const mtf_wave_color_15m = sanitizeElliottHexColor(raw.mtf_wave_color_15m, base.mtf_wave_color_15m);
  const mtf_label_color_4h = sanitizeElliottHexColor(raw.mtf_label_color_4h, base.mtf_label_color_4h);
  const mtf_label_color_1h = sanitizeElliottHexColor(raw.mtf_label_color_1h, base.mtf_label_color_1h);
  const mtf_label_color_15m = sanitizeElliottHexColor(raw.mtf_label_color_15m, base.mtf_label_color_15m);
  const show_line_4h = typeof raw.show_line_4h === "boolean" ? raw.show_line_4h : base.show_line_4h;
  const show_line_1h = typeof raw.show_line_1h === "boolean" ? raw.show_line_1h : base.show_line_1h;
  const show_line_15m = typeof raw.show_line_15m === "boolean" ? raw.show_line_15m : base.show_line_15m;
  const show_label_4h = typeof raw.show_label_4h === "boolean" ? raw.show_label_4h : base.show_label_4h;
  const show_label_1h = typeof raw.show_label_1h === "boolean" ? raw.show_label_1h : base.show_label_1h;
  const show_label_15m = typeof raw.show_label_15m === "boolean" ? raw.show_label_15m : base.show_label_15m;
  const lineStyle = (v: unknown, d: "solid" | "dotted" | "dashed") =>
    v === "solid" || v === "dotted" || v === "dashed" ? v : d;
  const lineWidth = (v: unknown, d: number) =>
    typeof v === "number" && Number.isFinite(v) ? Math.min(6, Math.max(1, Math.round(v))) : d;
  const mtf_line_style_4h = lineStyle(raw.mtf_line_style_4h, base.mtf_line_style_4h);
  const mtf_line_style_1h = lineStyle(raw.mtf_line_style_1h, base.mtf_line_style_1h);
  const mtf_line_style_15m = lineStyle(raw.mtf_line_style_15m, base.mtf_line_style_15m);
  const mtf_line_width_4h = lineWidth(raw.mtf_line_width_4h, base.mtf_line_width_4h);
  const mtf_line_width_1h = lineWidth(raw.mtf_line_width_1h, base.mtf_line_width_1h);
  const mtf_line_width_15m = lineWidth(raw.mtf_line_width_15m, base.mtf_line_width_15m);

  const show_zigzag_pivot_4h =
    typeof raw.show_zigzag_pivot_4h === "boolean" ? raw.show_zigzag_pivot_4h : base.show_zigzag_pivot_4h;
  const show_zigzag_pivot_1h =
    typeof raw.show_zigzag_pivot_1h === "boolean" ? raw.show_zigzag_pivot_1h : base.show_zigzag_pivot_1h;
  const show_zigzag_pivot_15m =
    typeof raw.show_zigzag_pivot_15m === "boolean" ? raw.show_zigzag_pivot_15m : base.show_zigzag_pivot_15m;

  const mtf_zigzag_color_4h = sanitizeElliottHexColor(raw.mtf_zigzag_color_4h, base.mtf_zigzag_color_4h);
  const mtf_zigzag_color_1h = sanitizeElliottHexColor(raw.mtf_zigzag_color_1h, base.mtf_zigzag_color_1h);
  const mtf_zigzag_color_15m = sanitizeElliottHexColor(raw.mtf_zigzag_color_15m, base.mtf_zigzag_color_15m);
  const mtf_zigzag_line_style_4h = lineStyle(raw.mtf_zigzag_line_style_4h, base.mtf_zigzag_line_style_4h);
  const mtf_zigzag_line_style_1h = lineStyle(raw.mtf_zigzag_line_style_1h, base.mtf_zigzag_line_style_1h);
  const mtf_zigzag_line_style_15m = lineStyle(raw.mtf_zigzag_line_style_15m, base.mtf_zigzag_line_style_15m);
  const mtf_zigzag_line_width_4h = lineWidth(raw.mtf_zigzag_line_width_4h, base.mtf_zigzag_line_width_4h);
  const mtf_zigzag_line_width_1h = lineWidth(raw.mtf_zigzag_line_width_1h, base.mtf_zigzag_line_width_1h);
  const mtf_zigzag_line_width_15m = lineWidth(raw.mtf_zigzag_line_width_15m, base.mtf_zigzag_line_width_15m);

  return {
    version: 1,
    enabled,
    formations,
    show_projection_4h,
    show_projection_1h,
    show_projection_15m,
    show_historical_waves,
    show_nested_formations,
    projection_bar_hop,
    projection_steps,
    show_projection_alt_scenario,
    use_acp_zigzag_swing,
    acp_zigzag_row_index,
    elliott_zigzag_depth,
    elliott_zigzag_depth_4h,
    elliott_zigzag_depth_1h,
    elliott_zigzag_depth_15m,
    swing_depth,
    max_pivot_windows,
    strict_wave4_overlap,
    pattern_menu,
    pattern_menu_by_tf,
    mtf_wave_color_4h,
    mtf_wave_color_1h,
    mtf_wave_color_15m,
    mtf_label_color_4h,
    mtf_label_color_1h,
    mtf_label_color_15m,
    show_line_4h,
    show_line_1h,
    show_line_15m,
    show_label_4h,
    show_label_1h,
    show_label_15m,
    mtf_line_style_4h,
    mtf_line_style_1h,
    mtf_line_style_15m,
    mtf_line_width_4h,
    mtf_line_width_1h,
    mtf_line_width_15m,
    show_zigzag_pivot_4h,
    show_zigzag_pivot_1h,
    show_zigzag_pivot_15m,
    mtf_zigzag_color_4h,
    mtf_zigzag_color_1h,
    mtf_zigzag_color_15m,
    mtf_zigzag_line_style_4h,
    mtf_zigzag_line_style_1h,
    mtf_zigzag_line_style_15m,
    mtf_zigzag_line_width_4h,
    mtf_zigzag_line_width_1h,
    mtf_zigzag_line_width_15m,
  };
}
