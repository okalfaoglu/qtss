/**
 * `app_config` anahtarı `elliott_wave` — web panel + GET `/analysis/elliott-wave-config`.
 */

import { DEFAULT_ELLIOTT_PATTERN_MENU, type ElliottPatternMenuToggles } from "./elliottPatternMenuCatalog";

export type { ElliottPatternMenuToggles };

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
  /** Projeksiyonda her adım = kaç mum süresi (Pine varsayılan 22). */
  projection_bar_hop: number;
  /** Kaç segment ileri (Pine 12; üst sınır 24). */
  projection_steps: number;
  /**
   * @deprecated ACP zigzag kanal/tarama ayarıdır; Elliott V2 ZigZag artık bunu kullanmaz.
   * Geriye dönük JSON uyumluluğu için saklanır.
   */
  use_acp_zigzag_swing: boolean;
  /** @deprecated ACP zigzag satırı; Elliott motoru ile ilişkili değildir. */
  acp_zigzag_row_index: number;
  /**
   * Elliott V2 ZigZag (fraktal pencere): her iki yanda kaç mum (TradingView ZigZag ile uyumlu).
   * ACP `zigzag[]` değişkeninden bağımsızdır.
   */
  elliott_zigzag_depth: number;
  /** @deprecated Eski alan; normalize `elliott_zigzag_depth` ile doldurulur. */
  swing_depth: number;
  max_pivot_windows: number;
  strict_wave4_overlap: boolean;
  /**
   * Menüdeki dalga türleri — düzeltme motoru hangi kalıpları deneyeceğini filtreler (varsayılan hepsi açık).
   */
  pattern_menu: ElliottPatternMenuToggles;
  /** MTF ZigZag + dalga çizgileri rengi (hex). */
  mtf_wave_color_4h: string;
  mtf_wave_color_1h: string;
  mtf_wave_color_15m: string;
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
  projection_bar_hop: 22,
  projection_steps: 12,
  use_acp_zigzag_swing: false,
  acp_zigzag_row_index: 0,
  elliott_zigzag_depth: 21,
  swing_depth: 21,
  max_pivot_windows: 120,
  strict_wave4_overlap: false,
  pattern_menu: { ...DEFAULT_ELLIOTT_PATTERN_MENU },
  mtf_wave_color_4h: DEFAULT_ELLIOTT_MTF_WAVE_COLORS["4h"],
  mtf_wave_color_1h: DEFAULT_ELLIOTT_MTF_WAVE_COLORS["1h"],
  mtf_wave_color_15m: DEFAULT_ELLIOTT_MTF_WAVE_COLORS["15m"],
};

function isRecord(x: unknown): x is Record<string, unknown> {
  return x !== null && typeof x === "object" && !Array.isArray(x);
}

export function normalizeElliottWaveConfig(raw: unknown): ElliottWaveConfig {
  const base = {
    ...DEFAULT_ELLIOTT_WAVE_CONFIG,
    formations: { ...DEFAULT_ELLIOTT_WAVE_CONFIG.formations },
    pattern_menu: { ...DEFAULT_ELLIOTT_PATTERN_MENU },
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

  const projection_bar_hop =
    typeof raw.projection_bar_hop === "number" && Number.isFinite(raw.projection_bar_hop)
      ? Math.min(100, Math.max(1, Math.floor(raw.projection_bar_hop)))
      : base.projection_bar_hop;

  const projection_steps =
    typeof raw.projection_steps === "number" && Number.isFinite(raw.projection_steps)
      ? Math.min(24, Math.max(1, Math.floor(raw.projection_steps)))
      : base.projection_steps;

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
  // UI sadeleştirme: ayrı "formations.impulse" checkbox'ı kaldırıldı; menüdeki motive_impulse tek kaynak.
  formations.impulse = pattern_menu.motive_impulse;

  const mtf_wave_color_4h = sanitizeElliottHexColor(raw.mtf_wave_color_4h, base.mtf_wave_color_4h);
  const mtf_wave_color_1h = sanitizeElliottHexColor(raw.mtf_wave_color_1h, base.mtf_wave_color_1h);
  const mtf_wave_color_15m = sanitizeElliottHexColor(raw.mtf_wave_color_15m, base.mtf_wave_color_15m);

  return {
    version: 1,
    enabled,
    formations,
    show_projection_4h,
    show_projection_1h,
    show_projection_15m,
    show_historical_waves,
    projection_bar_hop,
    projection_steps,
    use_acp_zigzag_swing,
    acp_zigzag_row_index,
    elliott_zigzag_depth,
    swing_depth,
    max_pivot_windows,
    strict_wave4_overlap,
    pattern_menu,
    mtf_wave_color_4h,
    mtf_wave_color_1h,
    mtf_wave_color_15m,
  };
}
