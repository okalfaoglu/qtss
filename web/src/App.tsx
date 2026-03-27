import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fetchBinanceKlinesAsChartRows } from "./api/binanceKlines";
import {
  backfillMarketBarsFromRest,
  fetchChartPatternsConfig,
  fetchConfigList,
  fetchElliottWaveConfig,
  fetchHealth,
  fetchMarketBarsRecent,
  oauthTokenPassword,
  scanChannelSix,
  upsertAppConfig,
  type ChannelSixRejectJson,
  type ChannelSixResponse,
} from "./api/client";
import { channelDrawingToOverlay } from "./lib/channelOverlayFromDrawing";
import { buildChannelScanPivotMarkers } from "./lib/channelScanMarkers";
import {
  buildMultiPatternOverlayFromScan,
  type PatternLayerOverlay,
  type MultiPatternChartOverlay,
} from "./lib/patternDrawingBatchOverlay";
import { mergeChartOhlcRowsByOpenTime } from "./lib/mergeChartOhlcRows";
import type { ChartOhlcRow } from "./lib/marketBarsToCandles";
import { chartOhlcRowsToScanBars, chartOhlcRowsSortedChrono } from "./lib/chartRowsToOhlcBars";
import { AcpTrendoscopeSettingsCard } from "./components/AcpTrendoscopeSettingsCard";
import { ChartToolbar, type ChartTool } from "./components/ChartToolbar";
import { ProfitCalculator } from "./components/ProfitCalculator";
import { MultiTimeframeLiveStrip } from "./components/MultiTimeframeLiveStrip";
import { ElliottWaveLegend } from "./components/ElliottWaveLegend";
import { ElliottPatternMenuPanel } from "./components/ElliottPatternMenuPanel";
import { ElliottWaveCard } from "./components/ElliottWaveCard";
import { TvChartPane } from "./components/TvChartPane";
import {
  DEFAULT_ELLIOTT_WAVE_CONFIG,
  ELLIOTT_WAVE_CONFIG_KEY,
  mtfWaveColorsFromConfig,
  normalizeElliottWaveConfig,
  type ElliottWaveConfig,
} from "./lib/elliottWaveAppConfig";
import { buildElliottLegendRows } from "./lib/elliottWaveLegend";
import {
  buildElliottProjectionOverlayV2,
  buildMtfFramesV2,
  runElliottEngineV2,
  v2ToChartOverlays,
  type OhlcV2,
} from "./lib/elliottEngineV2";
import {
  ACP_CHART_PATTERNS_CONFIG_KEY,
  acpConfigToChannelSixOptions,
  DEFAULT_ACP_CONFIG,
  normalizeAcpChartPatternsConfig,
  type AcpChartPatternsConfig,
} from "./lib/acpChartPatternsConfig";
import { acpOhlcWindowForScan } from "./lib/acpScanWindow";
import {
  chartUsesBinanceRestForOhlc,
  persistChartOhlcMode,
  readChartOhlcMode,
  type ChartOhlcMode,
} from "./lib/chartOhlcSource";
import { CHART_INTERVALS } from "./lib/chartIntervals";

type Theme = "dark" | "light";
type SettingsTab = "general" | "elliott" | "elliott_impulse" | "elliott_corrective" | "acp" | "setting";

/** V2 ham ZigZag overlay katmanları — adapter’daki `zigzagKind` ile eşleşir. */
type ElliottZigzagTfKey = "4h" | "1h" | "15m";
type ElliottZigzagTfVisibility = Record<ElliottZigzagTfKey, boolean>;

const DEFAULT_ELLIOTT_ZIGZAG_TF: ElliottZigzagTfVisibility = {
  "4h": true,
  "1h": true,
  "15m": true,
};

function keepElliottZigzagLayer(kind: PatternLayerOverlay["zigzagKind"], vis: ElliottZigzagTfVisibility): boolean {
  if (kind === "elliott_v2_zigzag_macro") return vis["4h"];
  if (kind === "elliott_v2_zigzag_intermediate") return vis["1h"];
  if (kind === "elliott_v2_zigzag_micro") return vis["15m"];
  return true;
}

/** V2 ham ZigZag çizgisi (itki/düzeltme katmanları değil). Elliott panel kapalıyken yalnız bunlar çizilir. */
function isV2RawZigzagKind(kind: PatternLayerOverlay["zigzagKind"] | undefined): boolean {
  return (
    kind === "elliott_v2_zigzag_macro" ||
    kind === "elliott_v2_zigzag_intermediate" ||
    kind === "elliott_v2_zigzag_micro"
  );
}

/** `<input type="color" />` için #RGB → #RRGGBB */
function elliottColorInputValue(hex: string): string {
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

type ChartDefaults = {
  exchange: string;
  segment: string;
  symbol: string;
  interval: string;
  limit: string;
};

function readChartDefaults(): ChartDefaults {
  return {
    exchange: import.meta.env.VITE_DEFAULT_EXCHANGE ?? "binance",
    segment: import.meta.env.VITE_DEFAULT_SEGMENT ?? "spot",
    symbol: (import.meta.env.VITE_DEFAULT_SYMBOL ?? "BTCUSDT").toUpperCase(),
    interval: import.meta.env.VITE_DEFAULT_INTERVAL ?? "15m",
    limit: String(import.meta.env.VITE_DEFAULT_BAR_LIMIT ?? "1000"),
  };
}

/** 0 = canlı mum poll kapalı. */
function readLivePollMs(): number {
  const raw = import.meta.env.VITE_LIVE_POLL_MS;
  if (raw === "0" || raw === "false") return 0;
  const n = parseInt(String(raw ?? "5000"), 10);
  return Number.isFinite(n) && n >= 0 ? n : 5000;
}

function channelSixRejectTr(reject: ChannelSixRejectJson | undefined): string {
  if (!reject) return "reject alanı yok (eski API?)";
  switch (reject.code) {
    case "insufficient_pivots":
      return `Zigzag pivot yetersiz (${reject.have_pivots ?? "?"}/${reject.need_pivots ?? 6})`;
    case "pivot_alternation":
      return "Son 6 pivot alterne değil";
    case "bar_ratio_upper":
      return "Bar oranı (üç tepe) limit dışı";
    case "bar_ratio_lower":
      return "Bar oranı (üç dip) limit dışı";
    case "inspect_upper":
      return "Üst sınır inspect geçmedi (Pine: score/total < 0.2)";
    case "inspect_lower":
      return "Alt sınır inspect geçmedi";
    case "pattern_not_allowed":
      return "Pattern id filtreye takıldı (allowed_pattern_ids)";
    case "overlap_ignored":
      return "Overlapping formasyon nedeniyle yok sayıldı (avoid_overlap)";
    case "duplicate_pivot_window":
      return "Aynı 5 pivot penceresi tekrar ettiği için yok sayıldı";
    case "last_pivot_direction":
      return "Son pivot yön filtresi uyuşmadı (allowed_last_pivot_directions)";
    case "size_filter":
      return "Boyut filtresi (SizeFilters.checkSize) geçmedi";
    case "entry_not_in_channel":
      return "Son kapanış kanal bandı dışında (ignoreIfEntryCrossed)";
    default:
      return reject.code;
  }
}

function readEnvHint(): { clientId: string; clientSecret: string; email: string; password: string } {
  return {
    clientId: import.meta.env.VITE_OAUTH_CLIENT_ID ?? "",
    clientSecret: import.meta.env.VITE_OAUTH_CLIENT_SECRET ?? "",
    email: import.meta.env.VITE_DEV_EMAIL ?? "",
    password: import.meta.env.VITE_DEV_PASSWORD ?? "",
  };
}

export default function App() {
  const defaults = readChartDefaults();
  const [theme, setTheme] = useState<Theme>(() => {
    if (typeof window === "undefined") return "dark";
    const s = localStorage.getItem("qtss-theme") as Theme | null;
    return s === "dark" || s === "light" ? s : "dark";
  });

  const [drawerOpen, setDrawerOpen] = useState(false);
  const [drawerTab, setDrawerTab] = useState<SettingsTab>("general");
  const [drawerSearch, setDrawerSearch] = useState("");
  const [elliottZigzagByTf, setElliottZigzagByTf] = useState<ElliottZigzagTfVisibility>(() => ({
    ...DEFAULT_ELLIOTT_ZIGZAG_TF,
  }));

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    localStorage.setItem("qtss-theme", theme);
  }, [theme]);

  const toggleTheme = useCallback(() => {
    setTheme((t) => (t === "dark" ? "light" : "dark"));
  }, []);

  const [health, setHealth] = useState<string>("…");
  const [token, setToken] = useState<string | null>(null);
  const [configPreview, setConfigPreview] = useState<string>("");
  const [error, setError] = useState<string>("");
  const [bars, setBars] = useState<ChartOhlcRow[] | null>(null);
  const [barsError, setBarsError] = useState<string>("");
  const [barsLoading, setBarsLoading] = useState(false);
  const [backfillLoading, setBackfillLoading] = useState(false);
  const [backfillNote, setBackfillNote] = useState<string>("");
  const [configLoading, setConfigLoading] = useState(false);
  const [barExchange, setBarExchange] = useState(defaults.exchange);
  const [barSegment, setBarSegment] = useState(defaults.segment);
  const [barSymbol, setBarSymbol] = useState(defaults.symbol);
  const [barInterval, setBarInterval] = useState(defaults.interval);
  const [barLimit, setBarLimit] = useState(defaults.limit);
  const [chartOhlcMode, setChartOhlcMode] = useState<ChartOhlcMode>(() => readChartOhlcMode());
  const [acpConfig, setAcpConfig] = useState<AcpChartPatternsConfig>(() => ({ ...DEFAULT_ACP_CONFIG }));
  const [acpConfigLoadErr, setAcpConfigLoadErr] = useState("");
  const [acpSaveErr, setAcpSaveErr] = useState("");
  const [acpSaveBusy, setAcpSaveBusy] = useState(false);
  const [channelScanLoading, setChannelScanLoading] = useState(false);
  const [channelScanJson, setChannelScanJson] = useState<string>("");
  const [channelScanError, setChannelScanError] = useState<string>("");
  const [lastChannelScan, setLastChannelScan] = useState<ChannelSixResponse | null>(null);
  const [channelScanSummary, setChannelScanSummary] = useState<string>("");
  const [channelScanHoverTitle, setChannelScanHoverTitle] = useState<string>("");
  /** Sembol/interval tam yükleme sonrası artar — TvChartPane yalnızca bu değişince `fitContent`. */
  const [chartFitKey, setChartFitKey] = useState(0);

  /** Aynı anda birden fazla yükleme: yalnızca son isteğin cevabı `setBars` uygular (BTC→ETH yarışı). */
  const chartLoadSeqRef = useRef(0);
  const livePollEpochRef = useRef(0);

  const [chartTool, setChartTool] = useState<ChartTool>("crosshair");
  const [clearDrawNonce, setClearDrawNonce] = useState(0);
  const [profitCalcOpen, setProfitCalcOpen] = useState(false);
  const [toolNote, setToolNote] = useState("");
  const [elliottConfig, setElliottConfig] = useState<ElliottWaveConfig>(() => ({
    ...DEFAULT_ELLIOTT_WAVE_CONFIG,
    formations: { ...DEFAULT_ELLIOTT_WAVE_CONFIG.formations },
    pattern_menu: { ...DEFAULT_ELLIOTT_WAVE_CONFIG.pattern_menu },
  }));
  const [elliottLoadErr, setElliottLoadErr] = useState("");
  const [elliottSaveErr, setElliottSaveErr] = useState("");
  const [elliottSaveBusy, setElliottSaveBusy] = useState(false);
  const [elliottRefreshBusy, setElliottRefreshBusy] = useState(false);
  const [elliottV2Frames, setElliottV2Frames] = useState<
    Partial<Record<"15m" | "1h" | "4h", OhlcV2[]>> | null
  >(null);
  const ohlcFromBinance = useMemo(
    () => chartUsesBinanceRestForOhlc(chartOhlcMode, token, barExchange, barSegment),
    [chartOhlcMode, token, barExchange, barSegment],
  );

  const lastBarClose = useMemo(() => {
    if (!bars?.length) return null;
    const chrono = chartOhlcRowsSortedChrono(bars);
    const last = chrono[chrono.length - 1];
    const c = parseFloat(String(last.close).replace(",", "."));
    return Number.isFinite(c) ? c : null;
  }, [bars]);

  /** Elliott V2 ZigZag — ACP zigzag’dan bağımsız (kanal/tarama ACP sekmesinde). */
  const elliottZigzagDepth = useMemo(() => {
    const raw = elliottConfig.elliott_zigzag_depth ?? elliottConfig.swing_depth ?? 21;
    const d = Math.floor(raw);
    return Math.min(100, Math.max(2, Number.isFinite(d) ? d : 21));
  }, [elliottConfig.elliott_zigzag_depth, elliottConfig.swing_depth]);

  const toOhlcV2 = useCallback((src: ChartOhlcRow[]): OhlcV2[] => {
    return chartOhlcRowsSortedChrono(src)
      .map((r) => {
        const t = Math.floor(new Date(r.open_time).getTime() / 1000);
        const o = parseFloat(String(r.open));
        const h = parseFloat(String(r.high));
        const l = parseFloat(String(r.low));
        const c = parseFloat(String(r.close));
        if (![t, o, h, l, c].every(Number.isFinite)) return null;
        return { t, o, h, l, c };
      })
      .filter((x): x is OhlcV2 => x !== null);
  }, []);

  const elliottMtfRangeKey = useMemo(() => {
    if (!bars?.length) return "";
    const ch = chartOhlcRowsSortedChrono(bars);
    const first = ch[0]?.open_time ?? "";
    const last = ch[ch.length - 1]?.open_time ?? "";
    return `${first}|${last}`;
  }, [bars]);

  useEffect(() => {
    /* MTF OHLC — Elliott V2 motoru + ham ZigZag; REST’te ana grafikle aynı open_time penceresi (TF hizası). */
    let alive = true;
    const run = async () => {
      try {
        if (ohlcFromBinance && !bars?.length) {
          if (alive) setElliottV2Frames(null);
          return;
        }
        const lim = Math.min(50_000, Math.max(120, parseInt(barLimit, 10) || 500));
        const intervals: Array<"15m" | "1h" | "4h"> = ["15m", "1h", "4h"];
        const byTf: Partial<Record<"15m" | "1h" | "4h", OhlcV2[]>> = {};
        const chronoMain = bars?.length ? chartOhlcRowsSortedChrono(bars) : [];
        const rangeStartMs =
          chronoMain.length > 0 ? new Date(chronoMain[0]!.open_time).getTime() : null;
        const rangeEndMs =
          chronoMain.length > 0
            ? new Date(chronoMain[chronoMain.length - 1]!.open_time).getTime()
            : null;
        const useAlignedWindow =
          ohlcFromBinance && rangeStartMs != null && rangeEndMs != null && Number.isFinite(rangeStartMs);

        for (const iv of intervals) {
          let rows: ChartOhlcRow[] = [];
          if (ohlcFromBinance) {
            if (useAlignedWindow) {
              rows = await fetchBinanceKlinesAsChartRows({
                symbol: barSymbol.trim(),
                interval: iv,
                startTimeMs: rangeStartMs!,
                endTimeMs: rangeEndMs!,
                accessToken: token,
                segment: barSegment.trim() || "spot",
              });
            } else {
              rows = await fetchBinanceKlinesAsChartRows({
                symbol: barSymbol.trim(),
                interval: iv,
                limit: lim,
                accessToken: token,
                segment: barSegment.trim() || "spot",
              });
            }
          } else if (token) {
            rows = await fetchMarketBarsRecent(token, {
              exchange: barExchange.trim(),
              segment: barSegment.trim(),
              symbol: barSymbol.trim().toUpperCase(),
              interval: iv,
              limit: lim,
            });
          }
          const o = toOhlcV2(rows);
          if (o.length) byTf[iv] = o;
        }

        if (alive) setElliottV2Frames(byTf);
      } catch {
        if (alive) setElliottV2Frames(null);
      }
    };
    void run();
    return () => {
      alive = false;
    };
  }, [
    barExchange,
    barLimit,
    barSegment,
    barSymbol,
    elliottMtfRangeKey,
    ohlcFromBinance,
    token,
    toOhlcV2,
  ]);

  const elliottV2Output = useMemo(() => {
    /* Motor çıktısı ZigZag çizgileri için de kullanılır; `enabled` kapalı olsa da üretilir. */
    if (!bars?.length) return null;
    const anchorRows = toOhlcV2(bars);
    if (!anchorRows.length) return null;
    const tf = barInterval === "4h" ? "4h" : barInterval === "1h" ? "1h" : "15m";
    const fallback = buildMtfFramesV2(anchorRows, tf);
    const byTimeframe =
      elliottV2Frames && Object.keys(elliottV2Frames).length ? elliottV2Frames : fallback;
    return runElliottEngineV2({
      byTimeframe,
      zigzag: { depth: elliottZigzagDepth, deviationPct: 0.35, backstep: 3 },
      maxWindows: elliottConfig.max_pivot_windows,
      patternToggles: elliottConfig.pattern_menu,
    });
  }, [
    barInterval,
    bars,
    elliottConfig.max_pivot_windows,
    elliottConfig.pattern_menu,
    elliottV2Frames,
    elliottZigzagDepth,
    toOhlcV2,
  ]);

  const elliottChartBundle = useMemo(() => {
    if (!elliottV2Output) return null;
    const full = v2ToChartOverlays(
      elliottV2Output,
      elliottConfig.pattern_menu,
      undefined,
      elliottConfig.show_historical_waves,
    );
    if (!elliottConfig.enabled) {
      return {
        layers: full.layers.filter((l) => isV2RawZigzagKind(l.zigzagKind)),
        waveLabels: [],
      };
    }
    return full;
  }, [
    elliottConfig.enabled,
    elliottConfig.mtf_wave_color_15m,
    elliottConfig.mtf_wave_color_1h,
    elliottConfig.mtf_wave_color_4h,
    elliottConfig.pattern_menu,
    elliottConfig.show_historical_waves,
    elliottV2Output,
  ]);

  const elliottProjectionLayers = useMemo((): PatternLayerOverlay[] => {
    if (!elliottConfig.enabled || !elliottConfig.pattern_menu.motive_impulse || !bars?.length || !elliottV2Output) {
      return [];
    }
    const rows = toOhlcV2(bars);
    if (!rows.length) return [];
    const wc = mtfWaveColorsFromConfig(elliottConfig);
    const opt = {
      barHop: elliottConfig.projection_bar_hop,
      maxSteps: elliottConfig.projection_steps,
    };
    const out: PatternLayerOverlay[] = [];
    const specs: Array<{ tf: "4h" | "1h" | "15m"; on: boolean }> = [
      { tf: "4h", on: elliottConfig.show_projection_4h },
      { tf: "1h", on: elliottConfig.show_projection_1h },
      { tf: "15m", on: elliottConfig.show_projection_15m },
    ];
    for (const { tf, on } of specs) {
      if (!on) continue;
      const built = buildElliottProjectionOverlayV2(
        elliottV2Output,
        rows,
        opt,
        elliottConfig.pattern_menu,
        wc[tf],
        tf,
      );
      if (built?.layers?.length) out.push(...built.layers);
    }
    return out;
  }, [
    bars,
    elliottConfig.enabled,
    elliottConfig.mtf_wave_color_15m,
    elliottConfig.mtf_wave_color_1h,
    elliottConfig.mtf_wave_color_4h,
    elliottConfig.pattern_menu.motive_impulse,
    elliottConfig.pattern_menu,
    elliottConfig.projection_bar_hop,
    elliottConfig.projection_steps,
    elliottConfig.show_projection_15m,
    elliottConfig.show_projection_1h,
    elliottConfig.show_projection_4h,
    elliottV2Output,
    toOhlcV2,
  ]);

  const anyElliottProjection = useMemo(
    () =>
      elliottConfig.show_projection_4h ||
      elliottConfig.show_projection_1h ||
      elliottConfig.show_projection_15m,
    [
      elliottConfig.show_projection_15m,
      elliottConfig.show_projection_1h,
      elliottConfig.show_projection_4h,
    ],
  );

  const elliottLegendRows = useMemo(() => {
    return buildElliottLegendRows(elliottV2Output, anyElliottProjection);
  }, [anyElliottProjection, elliottV2Output]);

  const multiOverlay = useMemo(() => {
    if (!lastChannelScan?.matched || !bars?.length) return null;
    const scanWindow = acpOhlcWindowForScan(bars, acpConfig.calculated_bars, acpConfig.scanning.repaint);
    const scanLen = Math.min(lastChannelScan.bar_count, scanWindow.length);
    const scanBars = scanLen > 0 ? scanWindow.slice(-scanLen) : scanWindow;
    const fromMatches = buildMultiPatternOverlayFromScan(lastChannelScan, scanBars, acpConfig.display);
    if (fromMatches) {
      return fromMatches;
    }
    if (lastChannelScan.drawing) {
      const ch = channelDrawingToOverlay(scanBars, lastChannelScan.drawing);
      if (ch) {
        const fallback: MultiPatternChartOverlay = {
          layers: [{ upper: ch.upper, lower: ch.lower, zigzag: [] }],
          pivotLabels: [],
          patternLabels: [],
        };
        return fallback;
      }
    }
    return fromMatches;
  }, [lastChannelScan, bars, acpConfig.display, acpConfig.calculated_bars, acpConfig.scanning.repaint]);

  const mergedPatternLayers = useMemo(() => {
    const acp = multiOverlay?.layers ?? [];
    const cap = 32;
    const elayersRaw: PatternLayerOverlay[] = elliottChartBundle?.layers ?? [];
    const elayers = elayersRaw.filter((l) => keepElliottZigzagLayer(l.zigzagKind, elliottZigzagByTf));
    const proj = elliottProjectionLayers;
    const eAll = proj.length ? [...elayers, ...proj] : [...elayers];
    if (!eAll.length) return acp.slice(0, cap);
    const room = Math.max(0, cap - eAll.length);
    return [...acp.slice(0, room), ...eAll].slice(0, cap);
  }, [elliottChartBundle?.layers, elliottProjectionLayers, multiOverlay?.layers, elliottZigzagByTf]);

  const mergedPivotLabelMarkers = useMemo(() => {
    const a = multiOverlay?.pivotLabels ?? [];
    const e = elliottChartBundle?.waveLabels ?? [];
    /** Projeksiyon etiketleri `patternLayers[].zigzagMarkers` ile çizgi serisinde (gelecek mumu yok). */
    return [...a, ...e].sort((x, y) => (x.time as number) - (y.time as number));
  }, [elliottChartBundle?.waveLabels, multiOverlay?.pivotLabels]);

  const pivotMarkers = useMemo(() => {
    if (!lastChannelScan?.matched || !lastChannelScan.outcome || !bars?.length) return [];
    if ((multiOverlay?.pivotLabels?.length ?? 0) > 0) return [];
    const scanWindow = acpOhlcWindowForScan(bars, acpConfig.calculated_bars, acpConfig.scanning.repaint);
    const scanLen = Math.min(lastChannelScan.bar_count, scanWindow.length);
    const scanBars = scanLen > 0 ? scanWindow.slice(-scanLen) : scanWindow;
    return buildChannelScanPivotMarkers(scanBars, lastChannelScan.outcome.pivots, theme);
  }, [lastChannelScan, bars, theme, multiOverlay?.pivotLabels?.length, acpConfig.calculated_bars, acpConfig.scanning.repaint]);

  const clearChannelScanUi = useCallback(() => {
    setLastChannelScan(null);
    setChannelScanSummary("");
    setChannelScanHoverTitle("");
    setChannelScanJson("");
    setChannelScanError("");
  }, []);

  const onChartToolSelect = useCallback((t: ChartTool) => {
    setChartTool(t);
    if (t === "calc") setProfitCalcOpen(true);
  }, []);

  const onClearDrawings = useCallback(() => {
    setClearDrawNonce((n) => n + 1);
    setToolNote("Çizimler temizlendi.");
    window.setTimeout(() => setToolNote(""), 4000);
  }, []);

  useEffect(() => {
    let c = true;
    fetchHealth()
      .then((j) => {
        if (c) setHealth(JSON.stringify(j));
      })
      .catch((e) => {
        if (c) setHealth(String(e));
      });
    return () => {
      c = false;
    };
  }, []);

  /**
   * OHLC kaynağı: `ohlcFromBinance` ise Binance spot REST (güncel mum); aksi halde JWT + `market_bars`.
   * Otomatik modda giriş + binance/spot → REST; diğer borsalar → DB.
   */
  const loadChartFromToolbar = useCallback(async () => {
    const seq = ++chartLoadSeqRef.current;
    clearChannelScanUi();
    setBarsError("");
    setBarsLoading(true);
    setBars(null);
    try {
      if (ohlcFromBinance) {
        const lim = Math.min(50_000, Math.max(10, parseInt(barLimit, 10) || 500));
        const rows = await fetchBinanceKlinesAsChartRows({
          symbol: barSymbol.trim(),
          interval: barInterval.trim(),
          limit: lim,
          accessToken: token,
          segment: barSegment.trim() || "spot",
        });
        if (seq !== chartLoadSeqRef.current) return;
        setBars(rows);
        setChartFitKey((k) => k + 1);
      } else if (token) {
        const lim = Math.min(5_000, Math.max(1, parseInt(barLimit, 10) || 24));
        const rows = await fetchMarketBarsRecent(token, {
          exchange: barExchange.trim(),
          segment: barSegment.trim(),
          symbol: barSymbol.trim().toUpperCase(),
          interval: barInterval.trim(),
          limit: lim,
        });
        if (seq !== chartLoadSeqRef.current) return;
        setBars(rows);
        setChartFitKey((k) => k + 1);
      } else {
        if (seq !== chartLoadSeqRef.current) return;
        setBarsError("OHLC kaynağı Veritabanı seçili — giriş yapın veya kaynağı Otomatik / Borsa yapın.");
      }
    } catch (e) {
      if (seq !== chartLoadSeqRef.current) return;
      setBarsError(String(e));
    } finally {
      if (seq === chartLoadSeqRef.current) {
        setBarsLoading(false);
      }
    }
  }, [token, barExchange, barSegment, barSymbol, barInterval, barLimit, clearChannelScanUi, ohlcFromBinance]);

  // Sembol/zaman dilimi değişince grafiği otomatik yenile (Yükle butonsuz akış).
  useEffect(() => {
    const t = window.setTimeout(() => {
      void loadChartFromToolbar();
    }, 300);
    return () => window.clearTimeout(t);
  }, [loadChartFromToolbar]);

  /** Son mumları periyodik çekip birleştir (kapanmamış mum + yeni bar). `VITE_LIVE_POLL_MS=0` ile kapatılır. */
  useEffect(() => {
    if (!bars?.length) return undefined;
    const pollMs = readLivePollMs();
    if (pollMs === 0) return undefined;
    const epoch = ++livePollEpochRef.current;
    const sym = barSymbol.trim();
    const iv = barInterval.trim();
    const run = async () => {
      if (epoch !== livePollEpochRef.current) return;
      if (typeof document !== "undefined" && document.hidden) return;
      try {
        if (ohlcFromBinance) {
          const tail = await fetchBinanceKlinesAsChartRows({
            symbol: sym,
            interval: iv,
            limit: 8,
            accessToken: token,
            segment: barSegment.trim() || "spot",
          });
          if (epoch !== livePollEpochRef.current) return;
          setBars((prev) => (prev?.length ? mergeChartOhlcRowsByOpenTime(prev, tail) : prev));
        } else if (token) {
          const lim = Math.min(150, Math.max(8, parseInt(barLimit, 10) || 24));
          const tail = await fetchMarketBarsRecent(token, {
            exchange: barExchange.trim(),
            segment: barSegment.trim(),
            symbol: sym.toUpperCase(),
            interval: iv,
            limit: lim,
          });
          if (epoch !== livePollEpochRef.current) return;
          setBars((prev) => (prev?.length ? mergeChartOhlcRowsByOpenTime(prev, tail) : prev));
        }
      } catch {
        /* ağ hatası — sessiz */
      }
    };
    const id = window.setInterval(() => void run(), pollMs);
    return () => {
      livePollEpochRef.current++;
      window.clearInterval(id);
    };
  }, [bars?.length, token, barSymbol, barInterval, barLimit, barExchange, barSegment, ohlcFromBinance]);

  const runChannelSixScan = async () => {
    if (!token || !bars?.length) return;
    setChannelScanError("");
    setChannelScanJson("");
    setChannelScanLoading(true);
    try {
      const chrono = chartOhlcRowsSortedChrono(bars);
      const cap = Math.min(acpConfig.calculated_bars, chrono.length);
      const capped = chrono.slice(-cap);
      const payload = chartOhlcRowsToScanBars(capped);
      const base = acpConfigToChannelSixOptions(acpConfig, theme);
      const res = await scanChannelSix(token, { bars: payload, ...(base as Record<string, unknown>) });
      setLastChannelScan(res);
      setChannelScanJson(JSON.stringify(res, null, 2));
      if (res.matched && res.outcome) {
        const id = res.outcome.scan.pattern_type_id;
        const name = res.pattern_name ?? `id ${id}`;
        const sk = res.outcome.pivot_tail_skip ?? 0;
        const skipNote = sk > 0 ? ` · pivot_skip ${sk}` : "";
        const lvl = res.outcome.zigzag_level ?? 0;
        const lvlNote = lvl > 0 ? ` · level ${lvl}` : "";
        const zzNote = res.used_zigzag ? ` · zg ${res.used_zigzag.length}/${res.used_zigzag.depth}` : "";
        const nMatch = res.pattern_matches?.length ?? 1;
        const multiNames =
          nMatch > 1
            ? (res.pattern_matches ?? [])
                .slice(0, 5)
                .map((m) => m.pattern_name ?? `id ${m.outcome.scan.pattern_type_id}`)
                .join(" · ")
            : "";
        const multiTail = nMatch > 5 ? "…" : "";
        const multi = nMatch > 1 ? ` · ${nMatch} formasyon (${multiNames}${multiTail})` : "";
        const hoverNames =
          res.pattern_matches?.map((m) => m.pattern_name ?? `id ${m.outcome.scan.pattern_type_id}`).join(" · ") ?? name;
        const repaintNote = acpConfig.scanning.repaint ? "" : " · kapanmış mum";
        setChannelScanHoverTitle(`Formasyonlar: ${hoverNames}`);
        setChannelScanSummary(
          `${name} · pick ${res.outcome.scan.pick_upper}/${res.outcome.scan.pick_lower} · zz ${res.outcome.zigzag_pivot_count} pivot${skipNote}${lvlNote}${zzNote}${multi}${repaintNote}`,
        );
      } else {
        setChannelScanHoverTitle("");
        setChannelScanSummary(
          `Eşleşme yok · ${channelSixRejectTr(res.reject)} · ${res.bar_count} mum · ${res.zigzag_pivot_count} zz pivot${acpConfig.scanning.repaint ? "" : " · kapanmış mum"}`,
        );
      }
    } catch (e) {
      setChannelScanError(String(e));
      setLastChannelScan(null);
      setChannelScanSummary("");
      setChannelScanHoverTitle("");
    } finally {
      setChannelScanLoading(false);
    }
  };

  const backfillFromRest = async () => {
    if (!token) return;
    clearChannelScanUi();
    setBackfillNote("");
    setBarsError("");
    setBackfillLoading(true);
    try {
      const lim = Math.min(1_000, Math.max(1, parseInt(barLimit, 10) || 500));
      const res = await backfillMarketBarsFromRest(token, {
        symbol: barSymbol.trim(),
        interval: barInterval.trim(),
        segment: barSegment.trim(),
        limit: lim,
      });
      setBackfillNote(`Backfill tamam: ${res.upserted} mum yazıldı (${res.source ?? "rest"}).`);
      await loadChartFromToolbar();
    } catch (e) {
      setBarsError(String(e));
    } finally {
      setBackfillLoading(false);
    }
  };

  const refreshAcpConfig = useCallback(async () => {
    if (!token) return;
    setAcpConfigLoadErr("");
    try {
      const raw = await fetchChartPatternsConfig(token);
      setAcpConfig(normalizeAcpChartPatternsConfig(raw));
    } catch (e) {
      setAcpConfigLoadErr(String(e));
    }
  }, [token]);

  const refreshElliottConfig = useCallback(async () => {
    if (!token) return;
    setElliottLoadErr("");
    setElliottRefreshBusy(true);
    try {
      const raw = await fetchElliottWaveConfig(token);
      setElliottConfig(normalizeElliottWaveConfig(raw));
    } catch (e) {
      setElliottLoadErr(String(e));
    } finally {
      setElliottRefreshBusy(false);
    }
  }, [token]);

  const refreshConfig = useCallback(async () => {
    if (!token) return;
    setError("");
    setConfigLoading(true);
    try {
      const cfg = await fetchConfigList(token);
      setConfigPreview(JSON.stringify(cfg, null, 2));
      await refreshElliottConfig();
      await refreshAcpConfig();
    } catch (e) {
      setError(String(e));
    } finally {
      setConfigLoading(false);
    }
  }, [token, refreshElliottConfig, refreshAcpConfig]);

  useEffect(() => {
    if (token) void refreshAcpConfig();
  }, [token, refreshAcpConfig]);

  useEffect(() => {
    if (token) void refreshElliottConfig();
  }, [token, refreshElliottConfig]);

  const saveAcpToDatabase = async () => {
    if (!token) return;
    setAcpSaveBusy(true);
    setAcpSaveErr("");
    try {
      await upsertAppConfig(token, {
        key: ACP_CHART_PATTERNS_CONFIG_KEY,
        value: acpConfig,
        description: "ACP [Trendoscope®] grafik formasyon taraması (web panel)",
      });
      await refreshAcpConfig();
    } catch (e) {
      setAcpSaveErr(String(e));
    } finally {
      setAcpSaveBusy(false);
    }
  };

  const saveElliottToDatabase = async () => {
    if (!token) return;
    setElliottSaveBusy(true);
    setElliottSaveErr("");
    try {
      await upsertAppConfig(token, {
        key: ELLIOTT_WAVE_CONFIG_KEY,
        value: elliottConfig,
        description: "Elliott Wave panel (web) — analiz, formasyonlar ve parametreler",
      });
      await refreshElliottConfig();
    } catch (e) {
      setElliottSaveErr(String(e));
    } finally {
      setElliottSaveBusy(false);
    }
  };

  const tryDevLogin = async () => {
    setError("");
    setConfigPreview("");
    const env = readEnvHint();
    if (!env.clientId || !env.clientSecret || !env.email || !env.password) {
      setError(
        "web/.env eksik veya boş. web/.env.example dosyasını .env olarak kopyalayın, CHANGEME alanlarını seed çıktısı ve admin parolası ile doldurun; dev sunucuyu yeniden başlatın.",
      );
      return;
    }
    try {
      const tok = await oauthTokenPassword(env);
      setToken(tok.access_token);
      const cfg = await fetchConfigList(tok.access_token);
      setConfigPreview(JSON.stringify(cfg, null, 2));
      /* Token state henüz bir sonraki render’a kadar güncellenmediği için Elliott/ACP’yi doğrudan tok ile yükle. */
      setElliottLoadErr("");
      try {
        const rawE = await fetchElliottWaveConfig(tok.access_token);
        setElliottConfig(normalizeElliottWaveConfig(rawE));
      } catch (e) {
        setElliottLoadErr(String(e));
      }
      setAcpConfigLoadErr("");
      try {
        const rawA = await fetchChartPatternsConfig(tok.access_token);
        setAcpConfig(normalizeAcpChartPatternsConfig(rawA));
      } catch (e) {
        setAcpConfigLoadErr(String(e));
      }
      await loadChartFromToolbar();
    } catch (e) {
      setError(String(e));
    }
  };

  const settingsQuery = drawerSearch.trim().toLocaleLowerCase("tr-TR");
  const matchesSetting = (...terms: string[]) =>
    settingsQuery.length === 0 ||
    terms.some((t) => t.toLocaleLowerCase("tr-TR").includes(settingsQuery));

  return (
    <div className="tv-root">
      <header className="tv-topstrip">
        <button
          type="button"
          className="tv-hamburger"
          aria-label="Menü — OAuth, barlar, sağlık"
          aria-expanded={drawerOpen}
          aria-controls="qtss-drawer"
          onClick={() => setDrawerOpen(true)}
        >
          <span className="tv-hamburger__bar" />
          <span className="tv-hamburger__bar" />
          <span className="tv-hamburger__bar" />
        </button>
        <div className="tv-topstrip__controls" aria-label="Sembol ve zaman dilimi">
          <input
            className="tv-topstrip__input mono"
            aria-label="Sembol"
            value={barSymbol}
            onChange={(e) => setBarSymbol(e.target.value.toUpperCase())}
            placeholder="BTCUSDT"
            maxLength={32}
          />
          <select
            className="tv-topstrip__select"
            aria-label="Zaman dilimi"
            value={barInterval}
            onChange={(e) => setBarInterval(e.target.value)}
          >
            {CHART_INTERVALS.map((iv) => (
              <option key={iv} value={iv}>
                {iv}
              </option>
            ))}
          </select>
          <select
            className="tv-topstrip__select"
            aria-label="OHLC veri kaynağı"
            value={chartOhlcMode}
            title="Otomatik: giriş + binance/spot → canlı REST; diğer borsa → DB. Borsa: her zaman Binance REST. DB: market_bars (JWT)."
            onChange={(e) => {
              const v = e.target.value as ChartOhlcMode;
              setChartOhlcMode(v);
              persistChartOhlcMode(v);
            }}
          >
            <option value="auto">OHLC otomatik</option>
            <option value="exchange">OHLC borsa</option>
            <option value="database">OHLC DB</option>
          </select>
        </div>
        <div className="tv-topstrip__symbol">
          <span className="muted">
            {ohlcFromBinance
              ? "Binance REST (canlı)"
              : token
                ? `${barExchange} / ${barSegment} · DB`
                : "OHLC DB — giriş gerekir"}
          </span>
          {bars && bars.length > 0 ? <span className="muted">{bars.length} mum</span> : null}
          {channelScanSummary ? (
            <span
              className="tv-topstrip__scan"
              title={channelScanHoverTitle || "Kanal taraması — çekmede tam JSON"}
            >
              {channelScanLoading ? "Taranıyor…" : channelScanSummary}
            </span>
          ) : null}
          {barsError ? <span className="err tv-topstrip__err" title={barsError}>{barsError.slice(0, 72)}{barsError.length > 72 ? "…" : ""}</span> : null}
          {toolNote ? <span className="muted">{toolNote}</span> : null}
        </div>
        <div className="tv-topstrip__actions">
          {token && bars && bars.length > 0 ? (
            <button
              type="button"
              className="theme-toggle"
              title="POST /api/v1/analysis/patterns/channel-six"
              disabled={channelScanLoading}
              onClick={() => void runChannelSixScan()}
            >
              {channelScanLoading ? "…" : "Kanal tara"}
            </button>
          ) : null}
          <button type="button" className="theme-toggle" onClick={toggleTheme}>
            {theme === "dark" ? "Açık" : "Koyu"}
          </button>
        </div>
      </header>

      <MultiTimeframeLiveStrip
        symbol={barSymbol}
        activeInterval={barInterval}
        accessToken={token}
        exchange={barExchange}
        segment={barSegment}
        ohlcFromBinanceRest={ohlcFromBinance}
      />

      <div className="tv-workspace">
        <aside className="tv-rail" aria-label="Grafik araçları">
          <ChartToolbar
            variant="vertical"
            active={chartTool}
            onSelect={onChartToolSelect}
            onClearDrawings={onClearDrawings}
          />
        </aside>
        <main className="tv-chart-host">
          <ElliottWaveLegend
            visible={
              !!elliottConfig.enabled &&
              elliottLegendRows.length > 0
            }
            rows={elliottLegendRows}
          />
          <TvChartPane
            bars={bars}
            theme={theme}
            fitSessionKey={chartFitKey}
            activeTool={chartTool}
            clearDrawNonce={clearDrawNonce}
            pivotMarkers={pivotMarkers}
            patternLayers={mergedPatternLayers.length ? mergedPatternLayers : null}
            pivotLabelMarkers={mergedPivotLabelMarkers.length ? mergedPivotLabelMarkers : null}
            patternLabelMarkers={multiOverlay?.patternLabels ?? null}
          />
        </main>
      </div>

      {drawerOpen ? (
        <>
          <div className="tv-drawer-scrim" role="presentation" onClick={() => setDrawerOpen(false)} />
          <aside id="qtss-drawer" className="tv-drawer" aria-modal="true" role="dialog" aria-label="QTSS panel">
            <div className="tv-drawer__head">
              <span>QTSS</span>
              <button type="button" className="tv-icon-btn" onClick={() => setDrawerOpen(false)} aria-label="Kapat">
                ×
              </button>
            </div>
            <div className="tv-drawer__body">
              <div className="tv-settings__quick-search">
                <input
                  className="tv-topstrip__input"
                  value={drawerSearch}
                  onChange={(e) => setDrawerSearch(e.target.value)}
                  placeholder="Ayar ara (örn. zigzag, projection, repaint)"
                  aria-label="Ayar arama"
                />
              </div>
              <div className="tv-settings__tabs" role="tablist" aria-label="Ayar sekmeleri">
                <button
                  type="button"
                  role="tab"
                  aria-selected={drawerTab === "general"}
                  className={`tv-settings__tab ${drawerTab === "general" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("general")}
                >
                  Genel
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={drawerTab === "elliott"}
                  className={`tv-settings__tab ${drawerTab === "elliott" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("elliott")}
                >
                  Elliott
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={drawerTab === "acp"}
                  className={`tv-settings__tab ${drawerTab === "acp" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("acp")}
                >
                  ACP
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={drawerTab === "setting"}
                  className={`tv-settings__tab ${drawerTab === "setting" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("setting")}
                >
                  Setting
                </button>
              </div>

              {drawerTab === "general" ? (
                <>
                  {matchesSetting("api sağlık", "health", "durum") ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">Durum</p>
                      <p className="muted" style={{ margin: 0 }}>API sağlık: <span className="mono">{health}</span></p>
                    </div>
                  ) : null}
                  {matchesSetting("oturum", "config", "giriş", "token") ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">Oturum ve Config</p>
                      <div style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap" }}>
                        <button type="button" className="theme-toggle" onClick={tryDevLogin}>Giriş dene</button>
                        <button type="button" className="theme-toggle" onClick={refreshConfig} disabled={!token || configLoading}>
                          {configLoading ? "Config…" : "Config yenile"}
                        </button>
                      </div>
                      {error ? <p className="err">{error}</p> : null}
                    </div>
                  ) : null}
                  {matchesSetting("tema", "theme") ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">Görünüm</p>
                      <button type="button" className="theme-toggle" onClick={toggleTheme}>
                        {theme === "dark" ? "Açık temaya geç" : "Koyu temaya geç"}
                      </button>
                    </div>
                  ) : null}
                </>
              ) : null}

              {drawerTab === "elliott" ? (
                <>
                  {matchesSetting(
                    "zigzag",
                    "göster",
                    "gizle",
                    "overlay",
                    "4h",
                    "1h",
                    "15m",
                    "timeframe",
                    "derinlik",
                    "depth",
                    "elliott",
                  ) ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">ZigZag görünümü (TF)</p>
                      <p className="muted" style={{ margin: "0 0 0.5rem", fontSize: "0.82rem" }}>
                        V2 ham ZigZag pivot çizgileri — her zaman dilimi ayrı açılıp kapatılabilir.
                      </p>
                      <div style={{ marginBottom: "0.65rem" }}>
                        <label className="muted" style={{ display: "flex", flexDirection: "column", gap: "0.35rem", fontSize: "0.82rem" }}>
                          <span>Elliott ZigZag derinliği (her iki yanda mum, varsayılan 21)</span>
                          <input
                            type="number"
                            min={2}
                            max={100}
                            className="tv-topstrip__input mono"
                            style={{ maxWidth: "7rem" }}
                            title="Fraktal penceresi; ACP zigzag’dan bağımsız; analiz kapalıyken de geçerlidir"
                            value={elliottConfig.elliott_zigzag_depth}
                            onChange={(e) => {
                              const n = parseInt(e.target.value, 10);
                              const z = Math.min(100, Math.max(2, Number.isFinite(n) ? n : 21));
                              setElliottConfig((prev) => ({
                                ...prev,
                                elliott_zigzag_depth: z,
                                swing_depth: z,
                              }));
                            }}
                          />
                        </label>
                        <p className="muted" style={{ margin: "0.35rem 0 0", fontSize: "0.78rem" }}>
                          Etkin: <span className="mono">{elliottZigzagDepth}</span>
                        </p>
                      </div>
                      <div className="tv-settings__zigzag-tf">
                        <label className="tv-settings__check">
                          <input
                            type="checkbox"
                            checked={elliottZigzagByTf["4h"]}
                            onChange={(e) =>
                              setElliottZigzagByTf((v: ElliottZigzagTfVisibility) => ({
                                ...v,
                                "4h": e.target.checked,
                              }))
                            }
                          />
                          4h (makro)
                        </label>
                        <label className="tv-settings__check">
                          <input
                            type="checkbox"
                            checked={elliottZigzagByTf["1h"]}
                            onChange={(e) =>
                              setElliottZigzagByTf((v: ElliottZigzagTfVisibility) => ({
                                ...v,
                                "1h": e.target.checked,
                              }))
                            }
                          />
                          1h (ara)
                        </label>
                        <label className="tv-settings__check">
                          <input
                            type="checkbox"
                            checked={elliottZigzagByTf["15m"]}
                            onChange={(e) =>
                              setElliottZigzagByTf((v: ElliottZigzagTfVisibility) => ({
                                ...v,
                                "15m": e.target.checked,
                              }))
                            }
                          />
                          15m (mikro)
                        </label>
                      </div>
                    </div>
                  ) : null}
                  {matchesSetting(
                    "dalga",
                    "impulse",
                    "diagonal",
                    "düzeltme",
                    "zigzag",
                    "flat",
                    "üçgen",
                    "triangle",
                    "w-x-y",
                    "karmaşık",
                    "elliott",
                    "fib",
                    "fibo",
                    "renk",
                    "color",
                  ) ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">Elliott dalga türleri</p>
                      <p className="muted" style={{ margin: "0 0 0.5rem", fontSize: "0.82rem" }}>
                        İşaretli türler grafikte ve motorda kullanılır; kapatılanlar çizilmez / aranmaz.
                      </p>
                      <ElliottPatternMenuPanel value={elliottConfig} onChange={setElliottConfig} />
                      <div
                        style={{
                          marginTop: "0.65rem",
                          paddingTop: "0.5rem",
                          borderTop: "1px solid var(--tv-border, rgba(255,255,255,0.08))",
                          display: "grid",
                          gridTemplateColumns: "repeat(3, 1fr)",
                          gap: "0.5rem",
                        }}
                      >
                        <label className="muted" style={{ fontSize: "0.72rem", display: "flex", flexDirection: "column", gap: "0.25rem" }}>
                          <span>4h · makro çizgi</span>
                          <input
                            type="color"
                            aria-label="4h Elliott rengi"
                            value={elliottColorInputValue(elliottConfig.mtf_wave_color_4h)}
                            onChange={(e) =>
                              setElliottConfig((c) => ({ ...c, mtf_wave_color_4h: e.target.value }))
                            }
                            style={{
                              width: "100%",
                              height: "2rem",
                              padding: 0,
                              border: "none",
                              cursor: "pointer",
                              background: "transparent",
                            }}
                          />
                        </label>
                        <label className="muted" style={{ fontSize: "0.72rem", display: "flex", flexDirection: "column", gap: "0.25rem" }}>
                          <span>1h · ara çizgi</span>
                          <input
                            type="color"
                            aria-label="1h Elliott rengi"
                            value={elliottColorInputValue(elliottConfig.mtf_wave_color_1h)}
                            onChange={(e) =>
                              setElliottConfig((c) => ({ ...c, mtf_wave_color_1h: e.target.value }))
                            }
                            style={{
                              width: "100%",
                              height: "2rem",
                              padding: 0,
                              border: "none",
                              cursor: "pointer",
                              background: "transparent",
                            }}
                          />
                        </label>
                        <label className="muted" style={{ fontSize: "0.72rem", display: "flex", flexDirection: "column", gap: "0.25rem" }}>
                          <span>15m · mikro çizgi</span>
                          <input
                            type="color"
                            aria-label="15m Elliott rengi"
                            value={elliottColorInputValue(elliottConfig.mtf_wave_color_15m)}
                            onChange={(e) =>
                              setElliottConfig((c) => ({ ...c, mtf_wave_color_15m: e.target.value }))
                            }
                            style={{
                              width: "100%",
                              height: "2rem",
                              padding: 0,
                              border: "none",
                              cursor: "pointer",
                              background: "transparent",
                            }}
                          />
                        </label>
                      </div>
                    </div>
                  ) : null}
                  {matchesSetting(
                    "elliott",
                    "impulse",
                    "corrective",
                    "zigzag",
                    "projection",
                    "itki",
                    "düzeltme",
                  ) ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">Elliott — genel ayarlar</p>
                      <p className="muted" style={{ margin: "0 0 0.5rem", fontSize: "0.82rem" }}>
                        İtki (1–5) ve düzeltme (2/4) için ayrıca «İtki dalgaları» ve «Düzeltme dalgaları»
                        sekmeleri vardır.
                      </p>
                      {token ? (
                        <div
                          className="tv-elliott-card__body"
                          style={{
                            marginTop: "0.5rem",
                            paddingTop: "0.5rem",
                            borderTop: "1px solid var(--tv-border, rgba(255,255,255,0.08))",
                          }}
                        >
                          <ElliottWaveCard
                            value={elliottConfig}
                            onChange={setElliottConfig}
                            bars={bars}
                            effectiveSwingDepth={elliottZigzagDepth}
                            v2Output={elliottV2Output}
                            loadErr={elliottLoadErr}
                            saveErr={elliottSaveErr}
                            saveBusy={elliottSaveBusy}
                            refreshBusy={elliottRefreshBusy}
                            onSaveToDb={() => void saveElliottToDatabase()}
                            onRefreshFromServer={() => void refreshElliottConfig()}
                          />
                        </div>
                      ) : (
                        <p className="muted">Elliott ayarlarını görmek için giriş yap.</p>
                      )}
                    </div>
                  ) : null}
                </>
              ) : null}

              {drawerTab === "elliott_impulse" ? (
                <>
                  {matchesSetting(
                    "itki",
                    "impulse",
                    "elliott",
                    "projeksiyon",
                    "zigzag",
                    "swing",
                    "pivot",
                    "formasyon",
                  ) ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">İtki dalgaları (1–5)</p>
                      <p className="muted" style={{ margin: "0 0 0.5rem", fontSize: "0.82rem" }}>
                        Trend yönündeki beş dalgalı itki çizimi, projeksiyon ve ilgili motor parametreleri.
                        Tam metin için «Elliott» sekmesindeki kurallar özetine bakın.
                      </p>
                      {token ? (
                        <div
                          className="tv-elliott-card__body"
                          style={{
                            marginTop: "0.35rem",
                            paddingTop: "0.5rem",
                            borderTop: "1px solid var(--tv-border, rgba(255,255,255,0.08))",
                          }}
                        >
                          <ElliottWaveCard
                            layout="impulse"
                            value={elliottConfig}
                            onChange={setElliottConfig}
                            bars={bars}
                            effectiveSwingDepth={elliottZigzagDepth}
                            v2Output={elliottV2Output}
                            loadErr={elliottLoadErr}
                            saveErr={elliottSaveErr}
                            saveBusy={elliottSaveBusy}
                            refreshBusy={elliottRefreshBusy}
                            onSaveToDb={() => void saveElliottToDatabase()}
                            onRefreshFromServer={() => void refreshElliottConfig()}
                          />
                        </div>
                      ) : (
                        <p className="muted">İtki dalgası ayarları için giriş yap.</p>
                      )}
                    </div>
                  ) : null}
                </>
              ) : null}

              {drawerTab === "elliott_corrective" ? (
                <>
                  {matchesSetting(
                    "düzeltme",
                    "corrective",
                    "abc",
                    "elliott",
                    "dalga 2",
                    "dalga 4",
                    "zigzag",
                    "swing",
                    "formasyon",
                  ) ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">Düzeltme dalgaları (2 ve 4)</p>
                      <p className="muted" style={{ margin: "0 0 0.5rem", fontSize: "0.82rem" }}>
                        Dalga 2 ve 4 içindeki A–B–C düzeltmeleri, itki sonrası büyük ABC ve dalga 4 örtüşme
                        kuralı. Tam metin için «Elliott» sekmesindeki kurallar özetine bakın.
                      </p>
                      {token ? (
                        <div
                          className="tv-elliott-card__body"
                          style={{
                            marginTop: "0.35rem",
                            paddingTop: "0.5rem",
                            borderTop: "1px solid var(--tv-border, rgba(255,255,255,0.08))",
                          }}
                        >
                          <ElliottWaveCard
                            layout="corrective"
                            value={elliottConfig}
                            onChange={setElliottConfig}
                            bars={bars}
                            effectiveSwingDepth={elliottZigzagDepth}
                            v2Output={elliottV2Output}
                            loadErr={elliottLoadErr}
                            saveErr={elliottSaveErr}
                            saveBusy={elliottSaveBusy}
                            refreshBusy={elliottRefreshBusy}
                            onSaveToDb={() => void saveElliottToDatabase()}
                            onRefreshFromServer={() => void refreshElliottConfig()}
                          />
                        </div>
                      ) : (
                        <p className="muted">Düzeltme dalgası ayarları için giriş yap.</p>
                      )}
                    </div>
                  ) : null}
                </>
              ) : null}

              {drawerTab === "acp" ? (
                <>
                  {matchesSetting("acp", "trendoscope", "scan", "repaint") ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">ACP Ayarları</p>
                      {token ? (
                        <>
                          {acpConfigLoadErr ? <p className="err">ACP config yüklenemedi: {acpConfigLoadErr}</p> : null}
                          {acpSaveErr ? <p className="err">ACP kayıt: {acpSaveErr}</p> : null}
                          <AcpTrendoscopeSettingsCard
                            value={acpConfig}
                            onChange={setAcpConfig}
                            onSaveToDb={() => void saveAcpToDatabase()}
                            saveBusy={acpSaveBusy}
                            saveHint="Başarılı kayıttan sonra tekrar yüklenir."
                          />
                          <div style={{ marginTop: "0.5rem" }}>
                            <button type="button" className="theme-toggle" onClick={() => void refreshAcpConfig()}>
                              ACP ayarlarını yenile
                            </button>
                          </div>
                        </>
                      ) : (
                        <p className="muted">ACP ayarları için giriş yap.</p>
                      )}
                    </div>
                  ) : null}
                  {token && matchesSetting("kanal", "channel", "scan", "pattern") ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">Kanal Taraması</p>
                      <button
                        type="button"
                        className="theme-toggle"
                        onClick={() => void runChannelSixScan()}
                        disabled={channelScanLoading || !bars?.length}
                      >
                        {channelScanLoading ? "Taranıyor…" : "Kanal taraması (ACP)"}
                      </button>
                      {channelScanError ? <p className="err">{channelScanError}</p> : null}
                    </div>
                  ) : null}
                </>
              ) : null}

              {drawerTab === "setting" ? (
                <>
                  {token && matchesSetting("market bars", "backfill", "exchange", "segment", "limit") ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">Market Bars</p>
                      <div className="tv-settings__fields">
                        <label>
                          <span className="muted">exchange</span>
                          <input className="mono" value={barExchange} onChange={(e) => setBarExchange(e.target.value)} />
                        </label>
                        <label>
                          <span className="muted">segment</span>
                          <input className="mono" value={barSegment} onChange={(e) => setBarSegment(e.target.value)} />
                        </label>
                        <label>
                          <span className="muted">limit</span>
                          <input className="mono" value={barLimit} onChange={(e) => setBarLimit(e.target.value)} />
                        </label>
                      </div>
                      <div style={{ marginTop: "0.5rem" }}>
                        <button
                          type="button"
                          className="theme-toggle"
                          onClick={backfillFromRest}
                          disabled={backfillLoading || barsLoading}
                        >
                          {backfillLoading ? "REST…" : "REST doldur"}
                        </button>
                      </div>
                      {backfillNote ? <p className="muted">{backfillNote}</p> : null}
                    </div>
                  ) : null}
                  {matchesSetting("config json", "token", "advanced", "gelişmiş") ? (
                    <details className="card tv-collapsible">
                      <summary className="tv-drawer__section-head">Gelişmiş</summary>
                      {token ? (
                        <>
                          <p className="muted" style={{ marginTop: "0.5rem" }}>access_token (kısaltılmış)</p>
                          <pre className="mono">{token.slice(0, 48)}…</pre>
                        </>
                      ) : null}
                      {configPreview ? (
                        <>
                          <p className="muted" style={{ marginTop: "0.5rem" }}>GET /api/v1/config</p>
                          <pre className="mono" style={{ maxHeight: "12rem", overflow: "auto" }}>{configPreview}</pre>
                        </>
                      ) : null}
                      {channelScanJson ? (
                        <>
                          <p className="muted" style={{ marginTop: "0.5rem" }}>Kanal tarama JSON</p>
                          <pre className="mono" style={{ maxHeight: "12rem", overflow: "auto" }}>{channelScanJson}</pre>
                        </>
                      ) : null}
                    </details>
                  ) : null}
                </>
              ) : null}
            </div>
          </aside>
        </>
      ) : null}

      {profitCalcOpen ? (
        <div className="tv-profit-calc--dock">
          <ProfitCalculator
            open
            onClose={() => {
              setProfitCalcOpen(false);
              setChartTool("crosshair");
            }}
            lastPrice={lastBarClose}
          />
        </div>
      ) : null}
    </div>
  );
}
