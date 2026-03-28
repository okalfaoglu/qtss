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
  fetchAuthMe,
  fetchEngineSnapshots,
  fetchEngineRangeSignals,
  fetchEngineSymbols,
  fetchNansenSnapshot,
  fetchNansenSetupsLatest,
  fetchPaperBalance,
  fetchPaperFills,
  postEngineSymbol,
  patchEngineSymbol,
  type EngineSnapshotJoinedApiRow,
  type EngineSymbolApiRow,
  type NansenSetupsLatestApiResponse,
  type NansenSnapshotApiRow,
  type PaperBalanceRow,
  type PaperFillRow,
  type RangeSignalEventApiRow,
} from "./api/client";
import { channelDrawingToOverlay } from "./lib/channelOverlayFromDrawing";
import { buildChannelScanPivotMarkers } from "./lib/channelScanMarkers";
import {
  buildMultiPatternOverlayFromScan,
  type PatternLayerOverlay,
  type MultiPatternChartOverlay,
} from "./lib/patternDrawingBatchOverlay";
import { ChannelScanMatchesTable } from "./components/ChannelScanMatchesTable";
import { mergeChartOhlcRowsByOpenTime } from "./lib/mergeChartOhlcRows";
import type { ChartOhlcRow } from "./lib/marketBarsToCandles";
import { chartOhlcRowsToScanBars, chartOhlcRowsSortedChrono } from "./lib/chartRowsToOhlcBars";
import { AcpTrendoscopeSettingsCard } from "./components/AcpTrendoscopeSettingsCard";
import { ChartToolbar, type ChartTool } from "./components/ChartToolbar";
import { ProfitCalculator } from "./components/ProfitCalculator";
import { MultiTimeframeLiveStrip } from "./components/MultiTimeframeLiveStrip";
import { ElliottWaveLegend } from "./components/ElliottWaveLegend";
import { ElliottWaveCard } from "./components/ElliottWaveCard";
import { TvChartPane } from "./components/TvChartPane";
import {
  DEFAULT_ELLIOTT_WAVE_CONFIG,
  ELLIOTT_WAVE_CONFIG_KEY,
  mergePatternMenuOrTf,
  mtfWaveColorsFromConfig,
  mtfZigzagColorsFromConfig,
  normalizeElliottWaveConfig,
  patternMenuForTf,
  type ElliottWaveConfig,
} from "./lib/elliottWaveAppConfig";
import {
  ELLIOTT_PATTERN_MENU_GROUPS,
  type ElliottPatternMenuToggles,
} from "./lib/elliottPatternMenuCatalog";
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
import {
  deriveOpenPositionFromRangeEvents,
  openPositionLayerFromRangeEvents,
} from "./lib/rangeOpenPositionLayer";
import { rangeSignalMarkersFromEvents } from "./lib/rangeSignalMarkers";
import { patternLayerFromDbTradingRange, sweepMarkersFromDbTradingRange } from "./lib/tradingRangeDbOverlay";
import { formatDashboardNumber, type SignalDashboardPayload } from "./lib/signalDashboardPayload";
import { canAdmin, canOps, type AuthSession } from "./lib/rbac";
import type { SeriesMarker, UTCTimestamp } from "lightweight-charts";

type Theme = "dark" | "light";
type SettingsTab =
  | "general"
  | "elliott"
  | "elliott_impulse"
  | "elliott_corrective"
  | "acp"
  | "engine"
  | "nansen"
  | "setting";

type ElliottLineStyle = "solid" | "dotted" | "dashed";

/** V2 ham ZigZag overlay katmanları — adapter’daki `zigzagKind` ile eşleşir. */
function keepElliottZigzagLayer(
  kind: PatternLayerOverlay["zigzagKind"],
  c: Pick<ElliottWaveConfig, "show_zigzag_pivot_4h" | "show_zigzag_pivot_1h" | "show_zigzag_pivot_15m">,
): boolean {
  if (kind === "elliott_v2_zigzag_macro") return c.show_zigzag_pivot_4h;
  if (kind === "elliott_v2_zigzag_intermediate") return c.show_zigzag_pivot_1h;
  if (kind === "elliott_v2_zigzag_micro") return c.show_zigzag_pivot_15m;
  return true;
}

/** `market_bars.segment` ile üst çubuk segmentini hizalar. */
function normalizeMarketSegment(segment: string): string {
  const s = segment.trim().toLowerCase();
  if (s === "futures" || s === "usdt_futures" || s === "fapi") return "futures";
  return s || "spot";
}

/** V2 ham ZigZag çizgisi (itki/düzeltme katmanları değil). Elliott panel kapalıyken yalnız bunlar çizilir. */
function isV2RawZigzagKind(kind: PatternLayerOverlay["zigzagKind"] | undefined): boolean {
  return (
    kind === "elliott_v2_zigzag_macro" ||
    kind === "elliott_v2_zigzag_intermediate" ||
    kind === "elliott_v2_zigzag_micro"
  );
}

function patchPatternMenuTf(
  c: ElliottWaveConfig,
  tf: "4h" | "1h" | "15m",
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
    limit: String(import.meta.env.VITE_DEFAULT_BAR_LIMIT ?? "5000"),
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
    case "ratio_diff":
      return "Eğim farkı (getRatioDiff / ratioDiff) eşiği aşıldı";
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

/** OAuth access token — Ctrl+F5 / tam yenilemede oturum kalsın diye `localStorage` (Çıkış ile silinir). */
const ACCESS_TOKEN_STORAGE_KEY = "qtss_access_token";

function readStoredAccessToken(): string | null {
  if (typeof window === "undefined") return null;
  try {
    const t = localStorage.getItem(ACCESS_TOKEN_STORAGE_KEY);
    return t != null && t.trim() !== "" ? t.trim() : null;
  } catch {
    return null;
  }
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
  const isElliottDrawerGroup =
    drawerTab === "elliott" || drawerTab === "elliott_impulse" || drawerTab === "elliott_corrective";
  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    localStorage.setItem("qtss-theme", theme);
  }, [theme]);

  const toggleTheme = useCallback(() => {
    setTheme((t) => (t === "dark" ? "light" : "dark"));
  }, []);

  const [health, setHealth] = useState<string>("…");
  const [token, setToken] = useState<string | null>(() => readStoredAccessToken());

  useEffect(() => {
    if (typeof window === "undefined") return;
    try {
      if (token) {
        localStorage.setItem(ACCESS_TOKEN_STORAGE_KEY, token);
      } else {
        localStorage.removeItem(ACCESS_TOKEN_STORAGE_KEY);
      }
    } catch {
      /* private mode, quota */
    }
  }, [token]);
  const [authSession, setAuthSession] = useState<AuthSession | null>(null);
  const [authMeErr, setAuthMeErr] = useState("");
  const [authMeLoading, setAuthMeLoading] = useState(false);
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
    pattern_menu_by_tf: {
      "4h": { ...DEFAULT_ELLIOTT_WAVE_CONFIG.pattern_menu_by_tf["4h"] },
      "1h": { ...DEFAULT_ELLIOTT_WAVE_CONFIG.pattern_menu_by_tf["1h"] },
      "15m": { ...DEFAULT_ELLIOTT_WAVE_CONFIG.pattern_menu_by_tf["15m"] },
    },
  }));
  const [elliottLoadErr, setElliottLoadErr] = useState("");
  const [elliottSaveErr, setElliottSaveErr] = useState("");
  const [elliottSaveBusy, setElliottSaveBusy] = useState(false);
  const [elliottRefreshBusy, setElliottRefreshBusy] = useState(false);
  const [engineSnapshots, setEngineSnapshots] = useState<EngineSnapshotJoinedApiRow[]>([]);
  const [engineRangeSignals, setEngineRangeSignals] = useState<RangeSignalEventApiRow[]>([]);
  const [engineSymbols, setEngineSymbols] = useState<EngineSymbolApiRow[]>([]);
  const [enginePanelErr, setEnginePanelErr] = useState("");
  const [engineFormSymbol, setEngineFormSymbol] = useState("");
  const [engineFormInterval, setEngineFormInterval] = useState("4h");
  const [engineFormBusy, setEngineFormBusy] = useState(false);
  const [engineListRefreshing, setEngineListRefreshing] = useState(false);
  const [nansenSnapshot, setNansenSnapshot] = useState<NansenSnapshotApiRow | null>(null);
  const [nansenSetupsLatest, setNansenSetupsLatest] = useState<NansenSetupsLatestApiResponse>({
    run: null,
    rows: [],
    setup_endpoint_missing: false,
  });
  const [nansenPanelErr, setNansenPanelErr] = useState("");
  const [nansenRefreshing, setNansenRefreshing] = useState(false);
  const [paperBalance, setPaperBalance] = useState<PaperBalanceRow | null>(null);
  const [paperFills, setPaperFills] = useState<PaperFillRow[]>([]);
  const [showDbTradingRangeLayer, setShowDbTradingRangeLayer] = useState(true);
  const [showDbSweepMarkers, setShowDbSweepMarkers] = useState(true);
  const [showDbRangeSignalMarkers, setShowDbRangeSignalMarkers] = useState(true);
  const [showDbOpenPositionLine, setShowDbOpenPositionLine] = useState(true);
  const [elliottV2Frames, setElliottV2Frames] = useState<
    Partial<Record<"15m" | "1h" | "4h", OhlcV2[]>> | null
  >(null);
  const ohlcFromBinance = useMemo(
    () => chartUsesBinanceRestForOhlc(chartOhlcMode, token, barExchange, barSegment),
    [chartOhlcMode, token, barExchange, barSegment],
  );

  const rbacIsAdmin = useMemo(
    () => (authSession ? canAdmin(authSession.roles) : false),
    [authSession],
  );
  const rbacIsOps = useMemo(() => (authSession ? canOps(authSession.roles) : false), [authSession]);

  useEffect(() => {
    if (!token) {
      setAuthSession(null);
      setAuthMeErr("");
      setAuthMeLoading(false);
      return;
    }
    let cancelled = false;
    setAuthMeLoading(true);
    setAuthMeErr("");
    void fetchAuthMe(token)
      .then((s) => {
        if (!cancelled) setAuthSession(s);
      })
      .catch((e) => {
        if (!cancelled) {
          setAuthSession(null);
          setAuthMeErr(String(e));
        }
      })
      .finally(() => {
        if (!cancelled) setAuthMeLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [token]);

  const lastBarClose = useMemo(() => {
    if (!bars?.length) return null;
    const chrono = chartOhlcRowsSortedChrono(bars);
    const last = chrono[chrono.length - 1];
    const c = parseFloat(String(last.close).replace(",", "."));
    return Number.isFinite(c) ? c : null;
  }, [bars]);

  /** Grafik intervali ile aynı TF’nin ZigZag derinliği (panel swing sayımı). */
  const chartElliottZigzagDepth = useMemo(() => {
    const raw =
      barInterval === "4h"
        ? elliottConfig.elliott_zigzag_depth_4h
        : barInterval === "1h"
          ? elliottConfig.elliott_zigzag_depth_1h
          : elliottConfig.elliott_zigzag_depth_15m;
    const d = Math.floor(raw);
    return Math.min(100, Math.max(2, Number.isFinite(d) ? d : 21));
  }, [
    barInterval,
    elliottConfig.elliott_zigzag_depth_4h,
    elliottConfig.elliott_zigzag_depth_1h,
    elliottConfig.elliott_zigzag_depth_15m,
  ]);

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
    /* Motor çıktısı ZigZag çizgileri için de kullanılır; `enabled` kapalı olsa da üretilir.
     * Kaynak: tam `bars` (açık mum dahil). ACP `repaint=false` penceresi yalnızca `acpOhlcWindowForScan` ile;
     * Elliott kasıtlı olarak etkilenmez. */
    if (!bars?.length) return null;
    const anchorRows = toOhlcV2(bars);
    if (!anchorRows.length) return null;
    const tf = barInterval === "4h" ? "4h" : barInterval === "1h" ? "1h" : "15m";
    const fallback = buildMtfFramesV2(anchorRows, tf);
    const byTimeframe =
      elliottV2Frames && Object.keys(elliottV2Frames).length ? elliottV2Frames : fallback;
    return runElliottEngineV2({
      byTimeframe,
      zigzag: {
        depth: elliottConfig.elliott_zigzag_depth_4h,
        deviationPct: 0.35,
        backstep: 3,
      },
      zigzagDepthByTimeframe: {
        "4h": Math.min(100, Math.max(2, Math.floor(elliottConfig.elliott_zigzag_depth_4h))),
        "1h": Math.min(100, Math.max(2, Math.floor(elliottConfig.elliott_zigzag_depth_1h))),
        "15m": Math.min(100, Math.max(2, Math.floor(elliottConfig.elliott_zigzag_depth_15m))),
      },
      maxWindows: elliottConfig.max_pivot_windows,
      patternTogglesByTf: elliottConfig.pattern_menu_by_tf,
    });
  }, [
    barInterval,
    bars,
    elliottConfig.elliott_zigzag_depth_15m,
    elliottConfig.elliott_zigzag_depth_1h,
    elliottConfig.elliott_zigzag_depth_4h,
    elliottConfig.max_pivot_windows,
    elliottConfig.pattern_menu_by_tf,
    elliottV2Frames,
    toOhlcV2,
  ]);

  const elliottChartBundle = useMemo(() => {
    if (!elliottV2Output) return null;
    const lineVisibility = {
      "4h": elliottConfig.show_line_4h,
      "1h": elliottConfig.show_line_1h,
      "15m": elliottConfig.show_line_15m,
    } as const;
    const labelVisibility = {
      "4h": elliottConfig.show_label_4h,
      "1h": elliottConfig.show_label_1h,
      "15m": elliottConfig.show_label_15m,
    } as const;
    const labelColors = {
      "4h": elliottConfig.mtf_label_color_4h,
      "1h": elliottConfig.mtf_label_color_1h,
      "15m": elliottConfig.mtf_label_color_15m,
    } as const;
    const lineStyles = {
      "4h": elliottConfig.mtf_line_style_4h,
      "1h": elliottConfig.mtf_line_style_1h,
      "15m": elliottConfig.mtf_line_style_15m,
    } as const;
    const lineWidths = {
      "4h": elliottConfig.mtf_line_width_4h,
      "1h": elliottConfig.mtf_line_width_1h,
      "15m": elliottConfig.mtf_line_width_15m,
    } as const;
    const zigzagVisibility = {
      "4h": elliottConfig.show_zigzag_pivot_4h,
      "1h": elliottConfig.show_zigzag_pivot_1h,
      "15m": elliottConfig.show_zigzag_pivot_15m,
    } as const;
    const zzColors = mtfZigzagColorsFromConfig(elliottConfig);
    const zigzagLineStyles = {
      "4h": elliottConfig.mtf_zigzag_line_style_4h,
      "1h": elliottConfig.mtf_zigzag_line_style_1h,
      "15m": elliottConfig.mtf_zigzag_line_style_15m,
    } as const;
    const zigzagLineWidths = {
      "4h": elliottConfig.mtf_zigzag_line_width_4h,
      "1h": elliottConfig.mtf_zigzag_line_width_1h,
      "15m": elliottConfig.mtf_zigzag_line_width_15m,
    } as const;
    const full = v2ToChartOverlays(
      elliottV2Output,
      elliottConfig.pattern_menu_by_tf,
      mtfWaveColorsFromConfig(elliottConfig),
      elliottConfig.show_historical_waves,
      {
        showLines: lineVisibility,
        showLabels: labelVisibility,
        labelColors,
        lineStyles,
        lineWidths,
        showZigzagPivots: zigzagVisibility,
        zigzagColors: zzColors,
        zigzagLineStyles,
        zigzagLineWidths,
        showNestedFormations: elliottConfig.show_nested_formations,
      },
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
    elliottConfig.mtf_label_color_15m,
    elliottConfig.mtf_label_color_1h,
    elliottConfig.mtf_label_color_4h,
    elliottConfig.mtf_line_style_15m,
    elliottConfig.mtf_line_style_1h,
    elliottConfig.mtf_line_style_4h,
    elliottConfig.mtf_line_width_15m,
    elliottConfig.mtf_line_width_1h,
    elliottConfig.mtf_line_width_4h,
    elliottConfig.pattern_menu_by_tf,
    elliottConfig.mtf_zigzag_color_15m,
    elliottConfig.mtf_zigzag_color_1h,
    elliottConfig.mtf_zigzag_color_4h,
    elliottConfig.mtf_zigzag_line_style_15m,
    elliottConfig.mtf_zigzag_line_style_1h,
    elliottConfig.mtf_zigzag_line_style_4h,
    elliottConfig.mtf_zigzag_line_width_15m,
    elliottConfig.mtf_zigzag_line_width_1h,
    elliottConfig.mtf_zigzag_line_width_4h,
    elliottConfig.show_zigzag_pivot_15m,
    elliottConfig.show_zigzag_pivot_1h,
    elliottConfig.show_zigzag_pivot_4h,
    elliottConfig.show_historical_waves,
    elliottConfig.show_nested_formations,
    elliottConfig.show_line_15m,
    elliottConfig.show_line_1h,
    elliottConfig.show_line_4h,
    elliottConfig.show_label_15m,
    elliottConfig.show_label_1h,
    elliottConfig.show_label_4h,
    elliottV2Output,
  ]);

  const elliottProjectionLayers = useMemo((): PatternLayerOverlay[] => {
    if (!elliottConfig.enabled || !bars?.length || !elliottV2Output) {
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
      {
        tf: "4h",
        on:
          elliottConfig.show_projection_4h &&
          elliottConfig.show_line_4h &&
          patternMenuForTf(elliottConfig, "4h").motive_impulse,
      },
      {
        tf: "1h",
        on:
          elliottConfig.show_projection_1h &&
          elliottConfig.show_line_1h &&
          patternMenuForTf(elliottConfig, "1h").motive_impulse,
      },
      {
        tf: "15m",
        on:
          elliottConfig.show_projection_15m &&
          elliottConfig.show_line_15m &&
          patternMenuForTf(elliottConfig, "15m").motive_impulse,
      },
    ];
    for (const { tf, on } of specs) {
      if (!on) continue;
      const built = buildElliottProjectionOverlayV2(
        elliottV2Output,
        rows,
        opt,
        patternMenuForTf(elliottConfig, tf),
        wc[tf],
        tf,
      );
      if (built?.layers?.length) {
        out.push(
          ...built.layers.map((layer) => ({
            ...layer,
            zigzagLineStyle:
              tf === "4h"
                ? elliottConfig.mtf_line_style_4h
                : tf === "1h"
                  ? elliottConfig.mtf_line_style_1h
                  : elliottConfig.mtf_line_style_15m,
            zigzagLineWidth:
              tf === "4h"
                ? elliottConfig.mtf_line_width_4h
                : tf === "1h"
                  ? elliottConfig.mtf_line_width_1h
                  : elliottConfig.mtf_line_width_15m,
          })),
        );
      }
    }
    return out;
  }, [
    bars,
    elliottConfig.enabled,
    elliottConfig.mtf_wave_color_15m,
    elliottConfig.mtf_wave_color_1h,
    elliottConfig.mtf_wave_color_4h,
    elliottConfig.pattern_menu_by_tf,
    elliottConfig.projection_bar_hop,
    elliottConfig.projection_steps,
    elliottConfig.show_projection_15m,
    elliottConfig.show_projection_1h,
    elliottConfig.show_projection_4h,
    elliottConfig.show_line_15m,
    elliottConfig.show_line_1h,
    elliottConfig.show_line_4h,
    elliottConfig.mtf_line_style_15m,
    elliottConfig.mtf_line_style_1h,
    elliottConfig.mtf_line_style_4h,
    elliottConfig.mtf_line_width_15m,
    elliottConfig.mtf_line_width_1h,
    elliottConfig.mtf_line_width_4h,
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

  const dbTradingRangeSnapshot = useMemo(() => {
    if (!engineSnapshots.length) return null;
    const ex = barExchange.trim().toLowerCase();
    const seg = normalizeMarketSegment(barSegment);
    const sym = barSymbol.trim().toUpperCase();
    const iv = barInterval.trim();
    return (
      engineSnapshots.find(
        (s) =>
          s.engine_kind === "trading_range" &&
          s.exchange.trim().toLowerCase() === ex &&
          normalizeMarketSegment(s.segment) === seg &&
          s.symbol.trim().toUpperCase() === sym &&
          s.interval.trim() === iv,
      ) ?? null
    );
  }, [engineSnapshots, barExchange, barSegment, barSymbol, barInterval]);

  const dbTradingRangeLayer = useMemo((): PatternLayerOverlay | null => {
    if (!showDbTradingRangeLayer || !bars?.length || !dbTradingRangeSnapshot) return null;
    return patternLayerFromDbTradingRange(bars, dbTradingRangeSnapshot.payload);
  }, [showDbTradingRangeLayer, bars, dbTradingRangeSnapshot]);

  const dbSignalDashboardSnapshot = useMemo(() => {
    if (!engineSnapshots.length) return null;
    const ex = barExchange.trim().toLowerCase();
    const seg = normalizeMarketSegment(barSegment);
    const sym = barSymbol.trim().toUpperCase();
    const iv = barInterval.trim();
    return (
      engineSnapshots.find(
        (s) =>
          s.engine_kind === "signal_dashboard" &&
          s.exchange.trim().toLowerCase() === ex &&
          normalizeMarketSegment(s.segment) === seg &&
          s.symbol.trim().toUpperCase() === sym &&
          s.interval.trim() === iv,
      ) ?? null
    );
  }, [engineSnapshots, barExchange, barSegment, barSymbol, barInterval]);

  const dbSweepMarkers = useMemo((): SeriesMarker<UTCTimestamp>[] => {
    if (!showDbSweepMarkers || !bars?.length || !dbTradingRangeSnapshot) return [];
    return sweepMarkersFromDbTradingRange(bars, dbTradingRangeSnapshot.payload);
  }, [showDbSweepMarkers, bars, dbTradingRangeSnapshot]);

  const engineChartRangeSignalEvents = useMemo(() => {
    if (!engineRangeSignals.length) return [];
    const ex = barExchange.trim().toLowerCase();
    const seg = normalizeMarketSegment(barSegment);
    const sym = barSymbol.trim().toUpperCase();
    const iv = barInterval.trim();
    return engineRangeSignals.filter(
      (e) =>
        e.exchange.trim().toLowerCase() === ex &&
        normalizeMarketSegment(e.segment) === seg &&
        e.symbol.trim().toUpperCase() === sym &&
        e.interval.trim() === iv,
    );
  }, [engineRangeSignals, barExchange, barSegment, barSymbol, barInterval]);

  const dbRangeSignalMarkers = useMemo((): SeriesMarker<UTCTimestamp>[] => {
    if (!showDbRangeSignalMarkers || !bars?.length) return [];
    return rangeSignalMarkersFromEvents(bars, engineChartRangeSignalEvents);
  }, [showDbRangeSignalMarkers, bars, engineChartRangeSignalEvents]);

  const dbOpenPositionLayer = useMemo((): PatternLayerOverlay | null => {
    if (!showDbOpenPositionLine || !bars?.length) return null;
    return openPositionLayerFromRangeEvents(bars, engineChartRangeSignalEvents);
  }, [showDbOpenPositionLine, bars, engineChartRangeSignalEvents]);

  const chartDerivedOpenPosition = useMemo(
    () => deriveOpenPositionFromRangeEvents(engineChartRangeSignalEvents),
    [engineChartRangeSignalEvents],
  );

  const chartRecentRangeEvents = useMemo(() => {
    return [...engineChartRangeSignalEvents]
      .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
      .slice(0, 5);
  }, [engineChartRangeSignalEvents]);

  const chartPatternLabelMarkers = useMemo((): SeriesMarker<UTCTimestamp>[] | null => {
    const acp = multiOverlay?.patternLabels ?? [];
    const merged = [...acp, ...dbSweepMarkers, ...dbRangeSignalMarkers].sort(
      (a, b) => (a.time as number) - (b.time as number),
    );
    return merged.length ? merged : null;
  }, [multiOverlay?.patternLabels, dbSweepMarkers, dbRangeSignalMarkers]);

  const mergedPatternLayers = useMemo(() => {
    const acp = multiOverlay?.layers ?? [];
    const cap = 32;
    const elayersRaw: PatternLayerOverlay[] = elliottChartBundle?.layers ?? [];
    const elayers = elayersRaw.filter((l) => keepElliottZigzagLayer(l.zigzagKind, elliottConfig));
    const proj = elliottProjectionLayers;
    const eAll = proj.length ? [...elayers, ...proj] : [...elayers];
    let inner: PatternLayerOverlay[];
    if (!eAll.length) inner = acp.slice(0, cap);
    else {
      const room = Math.max(0, cap - eAll.length);
      inner = [...acp.slice(0, room), ...eAll].slice(0, cap);
    }
    const dbPre: PatternLayerOverlay[] = [];
    if (dbTradingRangeLayer) dbPre.push(dbTradingRangeLayer);
    if (dbOpenPositionLayer) dbPre.push(dbOpenPositionLayer);
    if (dbPre.length) inner = [...dbPre, ...inner].slice(0, cap);
    return inner;
  }, [
    elliottChartBundle?.layers,
    elliottProjectionLayers,
    multiOverlay?.layers,
    elliottConfig.show_zigzag_pivot_4h,
    elliottConfig.show_zigzag_pivot_1h,
    elliottConfig.show_zigzag_pivot_15m,
    dbTradingRangeLayer,
    dbOpenPositionLayer,
  ]);

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

  const refreshEnginePanel = useCallback(async () => {
    if (!token) return;
    try {
      const [snaps, syms, sigs, pbal, pfills] = await Promise.all([
        fetchEngineSnapshots(token),
        fetchEngineSymbols(token),
        fetchEngineRangeSignals(token, { limit: 80 }),
        fetchPaperBalance(token).catch(() => null),
        fetchPaperFills(token, 15).catch(() => []),
      ]);
      setEngineSnapshots(snaps);
      setEngineSymbols(syms);
      setEngineRangeSignals(sigs);
      setPaperBalance(pbal);
      setPaperFills(pfills);
      setEnginePanelErr("");
    } catch (e) {
      setEnginePanelErr(String(e));
    }
  }, [token]);

  const refreshNansenPanel = useCallback(async () => {
    if (!token) return;
    const [snapRes, setupsRes] = await Promise.allSettled([
      fetchNansenSnapshot(token),
      fetchNansenSetupsLatest(token),
    ]);
    const errs: string[] = [];
    if (snapRes.status === "fulfilled") {
      setNansenSnapshot(snapRes.value);
    } else {
      setNansenSnapshot(null);
      errs.push(`snapshot: ${String(snapRes.reason)}`);
    }
    if (setupsRes.status === "fulfilled") {
      setNansenSetupsLatest(setupsRes.value);
    } else {
      setNansenSetupsLatest({ run: null, rows: [], setup_endpoint_missing: false });
      errs.push(`setups: ${String(setupsRes.reason)}`);
    }
    setNansenPanelErr(errs.join(" · "));
  }, [token]);

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

  useEffect(() => {
    if (!token) {
      setEngineSnapshots([]);
      setEngineRangeSignals([]);
      setEngineSymbols([]);
      setPaperBalance(null);
      setPaperFills([]);
      setEnginePanelErr("");
      return;
    }
    void refreshEnginePanel();
    const id = window.setInterval(() => {
      void refreshEnginePanel();
    }, 60_000);
    return () => window.clearInterval(id);
  }, [token, refreshEnginePanel]);

  useEffect(() => {
    if (!drawerOpen || drawerTab !== "nansen" || !token) return;
    void refreshNansenPanel();
    const id = window.setInterval(() => {
      void refreshNansenPanel();
    }, 90_000);
    return () => window.clearInterval(id);
  }, [drawerOpen, drawerTab, token, refreshNansenPanel]);

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
        const lim = Math.min(50_000, Math.max(1, parseInt(barLimit, 10) || 24));
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

  const runChannelSixScan = useCallback(async () => {
    if (!token || !bars?.length) return;
    setChannelScanError("");
    setChannelScanJson("");
    setChannelScanLoading(true);
    try {
      const scanWindow = acpOhlcWindowForScan(bars, acpConfig.calculated_bars, acpConfig.scanning.repaint);
      if (!scanWindow.length) {
        setChannelScanError(
          acpConfig.scanning.repaint
            ? "ACP taraması için yeterli mum yok."
            : "ACP taraması (repaint kapalı): en az iki kapanmış mum gerekir.",
        );
        setLastChannelScan(null);
        setChannelScanJson("");
        setChannelScanSummary("");
        setChannelScanHoverTitle("");
        return;
      }
      const payload = chartOhlcRowsToScanBars(scanWindow);
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
  }, [token, bars, acpConfig, theme]);

  const autoScanTfRef = useRef<string | null>(null);
  useEffect(() => {
    if (!acpConfig.scanning.auto_scan_on_timeframe_change) return;
    if (!token || !bars?.length) return;
    const prev = autoScanTfRef.current;
    autoScanTfRef.current = barInterval;
    if (prev !== null && prev !== barInterval) {
      void runChannelSixScan();
    }
  }, [barInterval, acpConfig.scanning.auto_scan_on_timeframe_change, token, bars?.length, runChannelSixScan]);

  const autoScanAwaitEnableRef = useRef(true);
  useEffect(() => {
    const on = acpConfig.scanning.auto_scan_on_timeframe_change;
    if (!on) {
      autoScanAwaitEnableRef.current = true;
      return;
    }
    if (!token || !bars?.length) return;
    if (autoScanAwaitEnableRef.current) {
      autoScanAwaitEnableRef.current = false;
      void runChannelSixScan();
    }
  }, [acpConfig.scanning.auto_scan_on_timeframe_change, token, bars?.length, runChannelSixScan]);

  const backfillFromRest = async () => {
    if (!token) return;
    clearChannelScanUi();
    setBackfillNote("");
    setBarsError("");
    setBackfillLoading(true);
    try {
      const lim = Math.min(50_000, Math.max(1, parseInt(barLimit, 10) || 500));
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
      if (authSession && canAdmin(authSession.roles)) {
        const cfg = await fetchConfigList(token);
        setConfigPreview(JSON.stringify(cfg, null, 2));
      } else {
        setConfigPreview(
          "(Tam `GET /api/v1/config` listesi yalnızca admin — Elliott/ACP sunucu ayarları aşağıda yenilenir.)",
        );
      }
      await refreshElliottConfig();
      await refreshAcpConfig();
    } catch (e) {
      setError(String(e));
    } finally {
      setConfigLoading(false);
    }
  }, [token, authSession, refreshElliottConfig, refreshAcpConfig]);

  useEffect(() => {
    if (token) void refreshAcpConfig();
  }, [token, refreshAcpConfig]);

  useEffect(() => {
    if (token) void refreshElliottConfig();
  }, [token, refreshElliottConfig]);

  const saveAcpToDatabase = async () => {
    if (!token || !authSession || !canAdmin(authSession.roles)) return;
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
    if (!token || !authSession || !canAdmin(authSession.roles)) return;
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
            patternLabelMarkers={chartPatternLabelMarkers}
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
                  aria-selected={isElliottDrawerGroup}
                  className={`tv-settings__tab ${isElliottDrawerGroup ? "is-active" : ""}`}
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
                  aria-selected={drawerTab === "engine"}
                  className={`tv-settings__tab ${drawerTab === "engine" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("engine")}
                >
                  Motor
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={drawerTab === "nansen"}
                  className={`tv-settings__tab ${drawerTab === "nansen" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("nansen")}
                >
                  Nansen
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
              {isElliottDrawerGroup ? (
                <div
                  className="tv-settings__tabs tv-settings__subtabs"
                  role="tablist"
                  aria-label="Elliott alt sekmeleri"
                >
                  <button
                    type="button"
                    role="tab"
                    aria-selected={drawerTab === "elliott"}
                    className={`tv-settings__tab ${drawerTab === "elliott" ? "is-active" : ""}`}
                    onClick={() => setDrawerTab("elliott")}
                  >
                    Özet
                  </button>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={drawerTab === "elliott_impulse"}
                    className={`tv-settings__tab ${drawerTab === "elliott_impulse" ? "is-active" : ""}`}
                    onClick={() => setDrawerTab("elliott_impulse")}
                  >
                    İtki (1–5)
                  </button>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={drawerTab === "elliott_corrective"}
                    className={`tv-settings__tab ${drawerTab === "elliott_corrective" ? "is-active" : ""}`}
                    onClick={() => setDrawerTab("elliott_corrective")}
                  >
                    Düzeltme (2/4)
                  </button>
                </div>
              ) : null}

              {drawerTab === "general" ? (
                <>
                  {matchesSetting("api sağlık", "health", "durum") ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">Durum</p>
                      <p className="muted" style={{ margin: 0 }}>API sağlık: <span className="mono">{health}</span></p>
                    </div>
                  ) : null}
                  {matchesSetting("oturum", "config", "giriş", "token", "rol", "rbac") ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">Oturum ve Config</p>
                      <p className="muted" style={{ fontSize: "0.75rem", marginBottom: "0.35rem" }}>
                        API erişimi JWT + sunucu RBAC ile sınırlıdır; aşağıdaki roller yalnızca arayüz rehberi içindir,
                        asıl kontrol uçlardadır.
                      </p>
                      <div style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap", alignItems: "center" }}>
                        <button type="button" className="theme-toggle" onClick={tryDevLogin}>
                          Giriş dene
                        </button>
                        <button type="button" className="theme-toggle" onClick={refreshConfig} disabled={!token || configLoading}>
                          {configLoading ? "Config…" : "Config yenile"}
                        </button>
                        <button
                          type="button"
                          className="theme-toggle"
                          disabled={!token}
                          onClick={() => {
                            setToken(null);
                            setAuthSession(null);
                            setConfigPreview("");
                            setAuthMeErr("");
                          }}
                        >
                          Çıkış
                        </button>
                      </div>
                      {authMeLoading ? <p className="muted" style={{ marginTop: "0.35rem" }}>Roller yükleniyor…</p> : null}
                      {authMeErr ? <p className="err" style={{ marginTop: "0.35rem" }}>{authMeErr}</p> : null}
                      {authSession ? (
                        <p className="muted mono" style={{ marginTop: "0.35rem", fontSize: "0.72rem", wordBreak: "break-all" }}>
                          userId={authSession.userId}
                          <br />
                          orgId={authSession.orgId}
                          <br />
                          roles={authSession.roles.length ? authSession.roles.join(", ") : "—"}
                          {rbacIsAdmin ? " · admin" : ""}
                          {rbacIsOps && !rbacIsAdmin ? " · ops (trader/admin)" : ""}
                          {authSession && !rbacIsOps ? " · salt okunur (viewer/analyst)" : ""}
                        </p>
                      ) : null}
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
                        Dalga türleri ve görünüm — TF başına. Motor ve çizim aynı anahtarları kullanır; ayarlar{" "}
                        <code className="mono">app_config.elliott_wave</code> ile veritabanına kaydedilir.
                      </p>
                      <div
                        style={{
                          marginTop: "0.65rem",
                          paddingTop: "0.5rem",
                          borderTop: "1px solid var(--tv-border, rgba(255,255,255,0.08))",
                          display: "grid",
                          gap: "0.5rem",
                        }}
                      >
                        <p className="muted" style={{ margin: 0, fontSize: "0.78rem" }}>
                          Sol sütun: dalga / katman türü; üstte timeframe: 4H / 1H / 15M. «ZigZag (depth)» her TF için
                          ZigZag fraktal penceresi (mum). «ZigZag (pivot)» ham pivot hattıdır; «Dalga çizgisi» itki ve
                          düzeltme segmentleri için geçerlidir.
                        </p>
                        <table style={{ width: "100%", borderCollapse: "collapse", fontSize: "0.78rem" }}>
                          <thead>
                            <tr>
                              <th style={{ textAlign: "left", padding: "0.25rem 0.2rem" }}>Ayar</th>
                              <th style={{ textAlign: "center", padding: "0.25rem 0.2rem" }}>4H</th>
                              <th style={{ textAlign: "center", padding: "0.25rem 0.2rem" }}>1H</th>
                              <th style={{ textAlign: "center", padding: "0.25rem 0.2rem" }}>15M</th>
                            </tr>
                          </thead>
                          <tbody>
                            {ELLIOTT_PATTERN_MENU_GROUPS.flatMap((g) => g.items).map((item) => (
                              <tr key={item.id}>
                                <td style={{ padding: "0.2rem" }}>
                                  <span style={{ fontWeight: 600 }}>{item.titleTr}</span>
                                  {item.structure ? (
                                    <span className="mono muted" style={{ fontSize: "0.68rem", marginLeft: "0.25rem" }}>
                                      {item.structure}
                                    </span>
                                  ) : null}
                                </td>
                                <td style={{ textAlign: "center" }}>
                                  <input
                                    type="checkbox"
                                    checked={elliottConfig.pattern_menu_by_tf["4h"][item.id]}
                                    onChange={(e) =>
                                      setElliottConfig((c) => patchPatternMenuTf(c, "4h", item.id, e.target.checked))
                                    }
                                  />
                                </td>
                                <td style={{ textAlign: "center" }}>
                                  <input
                                    type="checkbox"
                                    checked={elliottConfig.pattern_menu_by_tf["1h"][item.id]}
                                    onChange={(e) =>
                                      setElliottConfig((c) => patchPatternMenuTf(c, "1h", item.id, e.target.checked))
                                    }
                                  />
                                </td>
                                <td style={{ textAlign: "center" }}>
                                  <input
                                    type="checkbox"
                                    checked={elliottConfig.pattern_menu_by_tf["15m"][item.id]}
                                    onChange={(e) =>
                                      setElliottConfig((c) => patchPatternMenuTf(c, "15m", item.id, e.target.checked))
                                    }
                                  />
                                </td>
                              </tr>
                            ))}
                            <tr>
                              <td style={{ padding: "0.2rem" }}>ZigZag (depth)</td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="number"
                                  min={2}
                                  max={100}
                                  className="tv-topstrip__input mono"
                                  style={{ maxWidth: "4.25rem" }}
                                  title="ZigZag depth — 4H (her iki yanda mum)"
                                  value={elliottConfig.elliott_zigzag_depth_4h}
                                  onChange={(e) => {
                                    const n = parseInt(e.target.value, 10);
                                    const z = Math.min(100, Math.max(2, Number.isFinite(n) ? n : 21));
                                    setElliottConfig((prev) => ({
                                      ...prev,
                                      elliott_zigzag_depth_4h: z,
                                      elliott_zigzag_depth: z,
                                      swing_depth: z,
                                    }));
                                  }}
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="number"
                                  min={2}
                                  max={100}
                                  className="tv-topstrip__input mono"
                                  style={{ maxWidth: "4.25rem" }}
                                  title="ZigZag depth — 1H"
                                  value={elliottConfig.elliott_zigzag_depth_1h}
                                  onChange={(e) => {
                                    const n = parseInt(e.target.value, 10);
                                    const z = Math.min(100, Math.max(2, Number.isFinite(n) ? n : 21));
                                    setElliottConfig((prev) => ({ ...prev, elliott_zigzag_depth_1h: z }));
                                  }}
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="number"
                                  min={2}
                                  max={100}
                                  className="tv-topstrip__input mono"
                                  style={{ maxWidth: "4.25rem" }}
                                  title="ZigZag depth — 15M"
                                  value={elliottConfig.elliott_zigzag_depth_15m}
                                  onChange={(e) => {
                                    const n = parseInt(e.target.value, 10);
                                    const z = Math.min(100, Math.max(2, Number.isFinite(n) ? n : 21));
                                    setElliottConfig((prev) => ({ ...prev, elliott_zigzag_depth_15m: z }));
                                  }}
                                />
                              </td>
                            </tr>
                            <tr>
                              <td style={{ padding: "0.2rem" }}>ZigZag (pivot)</td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="checkbox"
                                  checked={elliottConfig.show_zigzag_pivot_4h}
                                  onChange={(e) => setElliottConfig((c) => ({ ...c, show_zigzag_pivot_4h: e.target.checked }))}
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="checkbox"
                                  checked={elliottConfig.show_zigzag_pivot_1h}
                                  onChange={(e) => setElliottConfig((c) => ({ ...c, show_zigzag_pivot_1h: e.target.checked }))}
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="checkbox"
                                  checked={elliottConfig.show_zigzag_pivot_15m}
                                  onChange={(e) => setElliottConfig((c) => ({ ...c, show_zigzag_pivot_15m: e.target.checked }))}
                                />
                              </td>
                            </tr>
                            <tr>
                              <td style={{ padding: "0.2rem" }}>ZigZag renk</td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="color"
                                  value={elliottColorInputValue(elliottConfig.mtf_zigzag_color_4h)}
                                  onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_zigzag_color_4h: e.target.value }))}
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="color"
                                  value={elliottColorInputValue(elliottConfig.mtf_zigzag_color_1h)}
                                  onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_zigzag_color_1h: e.target.value }))}
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="color"
                                  value={elliottColorInputValue(elliottConfig.mtf_zigzag_color_15m)}
                                  onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_zigzag_color_15m: e.target.value }))}
                                />
                              </td>
                            </tr>
                            <tr>
                              <td style={{ padding: "0.2rem" }}>ZigZag cizgi tipi</td>
                              <td style={{ textAlign: "center" }}>
                                <select
                                  value={elliottConfig.mtf_zigzag_line_style_4h}
                                  onChange={(e) =>
                                    setElliottConfig((c) => ({
                                      ...c,
                                      mtf_zigzag_line_style_4h: e.target.value as ElliottLineStyle,
                                    }))
                                  }
                                >
                                  <option value="solid">Duz</option>
                                  <option value="dotted">Nokta</option>
                                  <option value="dashed">Kesik</option>
                                </select>
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <select
                                  value={elliottConfig.mtf_zigzag_line_style_1h}
                                  onChange={(e) =>
                                    setElliottConfig((c) => ({
                                      ...c,
                                      mtf_zigzag_line_style_1h: e.target.value as ElliottLineStyle,
                                    }))
                                  }
                                >
                                  <option value="solid">Duz</option>
                                  <option value="dotted">Nokta</option>
                                  <option value="dashed">Kesik</option>
                                </select>
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <select
                                  value={elliottConfig.mtf_zigzag_line_style_15m}
                                  onChange={(e) =>
                                    setElliottConfig((c) => ({
                                      ...c,
                                      mtf_zigzag_line_style_15m: e.target.value as ElliottLineStyle,
                                    }))
                                  }
                                >
                                  <option value="solid">Duz</option>
                                  <option value="dotted">Nokta</option>
                                  <option value="dashed">Kesik</option>
                                </select>
                              </td>
                            </tr>
                            <tr>
                              <td style={{ padding: "0.2rem" }}>ZigZag kalinligi</td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="number"
                                  min={1}
                                  max={6}
                                  value={elliottConfig.mtf_zigzag_line_width_4h}
                                  onChange={(e) =>
                                    setElliottConfig((c) => ({
                                      ...c,
                                      mtf_zigzag_line_width_4h: Math.min(6, Math.max(1, parseInt(e.target.value, 10) || 1)),
                                    }))
                                  }
                                  style={{ width: "3.3rem" }}
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="number"
                                  min={1}
                                  max={6}
                                  value={elliottConfig.mtf_zigzag_line_width_1h}
                                  onChange={(e) =>
                                    setElliottConfig((c) => ({
                                      ...c,
                                      mtf_zigzag_line_width_1h: Math.min(6, Math.max(1, parseInt(e.target.value, 10) || 1)),
                                    }))
                                  }
                                  style={{ width: "3.3rem" }}
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="number"
                                  min={1}
                                  max={6}
                                  value={elliottConfig.mtf_zigzag_line_width_15m}
                                  onChange={(e) =>
                                    setElliottConfig((c) => ({
                                      ...c,
                                      mtf_zigzag_line_width_15m: Math.min(6, Math.max(1, parseInt(e.target.value, 10) || 1)),
                                    }))
                                  }
                                  style={{ width: "3.3rem" }}
                                />
                              </td>
                            </tr>
                            <tr>
                              <td style={{ padding: "0.2rem" }}>Dalga cizgisi</td>
                              <td style={{ textAlign: "center" }}><input type="checkbox" checked={elliottConfig.show_line_4h} onChange={(e) => setElliottConfig((c) => ({ ...c, show_line_4h: e.target.checked }))} /></td>
                              <td style={{ textAlign: "center" }}><input type="checkbox" checked={elliottConfig.show_line_1h} onChange={(e) => setElliottConfig((c) => ({ ...c, show_line_1h: e.target.checked }))} /></td>
                              <td style={{ textAlign: "center" }}><input type="checkbox" checked={elliottConfig.show_line_15m} onChange={(e) => setElliottConfig((c) => ({ ...c, show_line_15m: e.target.checked }))} /></td>
                            </tr>
                            <tr>
                              <td style={{ padding: "0.2rem" }}>Etiket</td>
                              <td style={{ textAlign: "center" }}><input type="checkbox" checked={elliottConfig.show_label_4h} onChange={(e) => setElliottConfig((c) => ({ ...c, show_label_4h: e.target.checked }))} /></td>
                              <td style={{ textAlign: "center" }}><input type="checkbox" checked={elliottConfig.show_label_1h} onChange={(e) => setElliottConfig((c) => ({ ...c, show_label_1h: e.target.checked }))} /></td>
                              <td style={{ textAlign: "center" }}><input type="checkbox" checked={elliottConfig.show_label_15m} onChange={(e) => setElliottConfig((c) => ({ ...c, show_label_15m: e.target.checked }))} /></td>
                            </tr>
                            <tr>
                              <td style={{ padding: "0.2rem" }}>Cizgi renk</td>
                              <td style={{ textAlign: "center" }}><input type="color" value={elliottColorInputValue(elliottConfig.mtf_wave_color_4h)} onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_wave_color_4h: e.target.value }))} /></td>
                              <td style={{ textAlign: "center" }}><input type="color" value={elliottColorInputValue(elliottConfig.mtf_wave_color_1h)} onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_wave_color_1h: e.target.value }))} /></td>
                              <td style={{ textAlign: "center" }}><input type="color" value={elliottColorInputValue(elliottConfig.mtf_wave_color_15m)} onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_wave_color_15m: e.target.value }))} /></td>
                            </tr>
                            <tr>
                              <td style={{ padding: "0.2rem" }}>Etiket renk</td>
                              <td style={{ textAlign: "center" }}><input type="color" value={elliottColorInputValue(elliottConfig.mtf_label_color_4h)} onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_label_color_4h: e.target.value }))} /></td>
                              <td style={{ textAlign: "center" }}><input type="color" value={elliottColorInputValue(elliottConfig.mtf_label_color_1h)} onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_label_color_1h: e.target.value }))} /></td>
                              <td style={{ textAlign: "center" }}><input type="color" value={elliottColorInputValue(elliottConfig.mtf_label_color_15m)} onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_label_color_15m: e.target.value }))} /></td>
                            </tr>
                            <tr>
                              <td style={{ padding: "0.2rem" }}>Cizgi tipi</td>
                              <td style={{ textAlign: "center" }}><select value={elliottConfig.mtf_line_style_4h} onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_line_style_4h: e.target.value as ElliottLineStyle }))}><option value="solid">Duz</option><option value="dotted">Nokta</option><option value="dashed">Kesik</option></select></td>
                              <td style={{ textAlign: "center" }}><select value={elliottConfig.mtf_line_style_1h} onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_line_style_1h: e.target.value as ElliottLineStyle }))}><option value="solid">Duz</option><option value="dotted">Nokta</option><option value="dashed">Kesik</option></select></td>
                              <td style={{ textAlign: "center" }}><select value={elliottConfig.mtf_line_style_15m} onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_line_style_15m: e.target.value as ElliottLineStyle }))}><option value="solid">Duz</option><option value="dotted">Nokta</option><option value="dashed">Kesik</option></select></td>
                            </tr>
                            <tr>
                              <td style={{ padding: "0.2rem" }}>Cizgi kalinligi</td>
                              <td style={{ textAlign: "center" }}><input type="number" min={1} max={6} value={elliottConfig.mtf_line_width_4h} onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_line_width_4h: Math.min(6, Math.max(1, parseInt(e.target.value, 10) || 1)) }))} style={{ width: "3.3rem" }} /></td>
                              <td style={{ textAlign: "center" }}><input type="number" min={1} max={6} value={elliottConfig.mtf_line_width_1h} onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_line_width_1h: Math.min(6, Math.max(1, parseInt(e.target.value, 10) || 1)) }))} style={{ width: "3.3rem" }} /></td>
                              <td style={{ textAlign: "center" }}><input type="number" min={1} max={6} value={elliottConfig.mtf_line_width_15m} onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_line_width_15m: Math.min(6, Math.max(1, parseInt(e.target.value, 10) || 1)) }))} style={{ width: "3.3rem" }} /></td>
                            </tr>
                          </tbody>
                        </table>
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
                            effectiveSwingDepth={chartElliottZigzagDepth}
                            v2Output={elliottV2Output}
                            loadErr={elliottLoadErr}
                            saveErr={elliottSaveErr}
                            saveBusy={elliottSaveBusy}
                            refreshBusy={elliottRefreshBusy}
                            onSaveToDb={() => void saveElliottToDatabase()}
                            onRefreshFromServer={() => void refreshElliottConfig()}
                            allowPersistConfig={rbacIsAdmin}
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
                            effectiveSwingDepth={chartElliottZigzagDepth}
                            v2Output={elliottV2Output}
                            loadErr={elliottLoadErr}
                            saveErr={elliottSaveErr}
                            saveBusy={elliottSaveBusy}
                            refreshBusy={elliottRefreshBusy}
                            onSaveToDb={() => void saveElliottToDatabase()}
                            onRefreshFromServer={() => void refreshElliottConfig()}
                            allowPersistConfig={rbacIsAdmin}
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
                            effectiveSwingDepth={chartElliottZigzagDepth}
                            v2Output={elliottV2Output}
                            loadErr={elliottLoadErr}
                            saveErr={elliottSaveErr}
                            saveBusy={elliottSaveBusy}
                            refreshBusy={elliottRefreshBusy}
                            onSaveToDb={() => void saveElliottToDatabase()}
                            onRefreshFromServer={() => void refreshElliottConfig()}
                            allowPersistConfig={rbacIsAdmin}
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
                            allowPersistConfig={rbacIsAdmin}
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
                      <p className="muted" style={{ fontSize: "0.8rem", marginBottom: "0.5rem" }}>
                        Manuel buton yok. ACP ayarlarında{" "}
                        <strong>Timeframe değişince otomatik kanal taraması</strong> ile üst şerit interval her
                        değiştiğinde tarama çalışır; açılışta veya seçeneği açtığınızda da bir kez tetiklenir.
                      </p>
                      {channelScanLoading ? <p className="muted">Taranıyor…</p> : null}
                      {channelScanError ? <p className="err">{channelScanError}</p> : null}
                      {lastChannelScan?.matched ? <ChannelScanMatchesTable res={lastChannelScan} /> : null}
                      {channelScanSummary ? (
                        <p className="muted" style={{ fontSize: "0.75rem", marginTop: "0.5rem" }}>
                          Özet: üst şerit. Geçmiş pencereler için ACP ayarlarında{" "}
                          <strong>Max patterns</strong> ve <strong>pivot_tail_skip_max</strong> artırın; veri derinliği için{" "}
                          <strong>Calculated bars</strong> ve grafik <strong>limit</strong>.
                        </p>
                      ) : null}
                    </div>
                  ) : null}
                </>
              ) : null}

              {drawerTab === "engine" ? (
                <>
                  {matchesSetting("motor", "engine", "snapshot", "trading", "range", "sembol", "worker") ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">Arka plan motor — DB snapshot</p>
                      <p className="muted" style={{ fontSize: "0.78rem", marginBottom: "0.5rem" }}>
                        <code>qtss-worker</code> tablodaki <code>engine_symbols</code> satırlarını okur, mumları yalnız{" "}
                        <strong>
                          <code>market_bars</code>
                        </strong>{" "}
                        tablosundan çeker; <code>trading_range</code> / <code>signal_dashboard</code> sonuçlarını{" "}
                        <code>analysis_snapshots</code>’a yazar. Bu panel API’den okur (otomatik ~60 sn); snapshot’ı üreten
                        worker ayrı süreçtir — tick süresi{" "}
                        <code>QTSS_ENGINE_TICK_SECS</code> (varsayılan 120 sn).
                      </p>
                      <ul className="muted" style={{ fontSize: "0.72rem", margin: "0 0 0.55rem 1rem", lineHeight: 1.45 }}>
                        <li>
                          <strong>Veri:</strong> İlgili exchange/segment/symbol/interval için <code>market_bars</code>{" "}
                          dolu olmalı (Ayarlar → Market Bars backfill veya worker’da <code>DATABASE_URL</code> +{" "}
                          <code>QTSS_KLINE_SYMBOL</code> ile canlı mum yazımı).
                        </li>
                        <li>
                          <strong>Worker:</strong> <code>qtss-worker</code> çalışmalı ve <code>DATABASE_URL</code> tanımlı
                          olmalı; aksi halde snapshot ve range olayı oluşmaz.
                        </li>
                        <li>
                          <strong>Grafik eşlemesi:</strong> Üst çubuktaki sembol ve interval, <code>engine_symbols</code>{" "}
                          satırıyla birebir aynı olmalı (ör. <code>15m</code> ile <code>4h</code> farklı hedeftir).
                        </li>
                        <li>
                          <strong>Paper (F4):</strong> Dry emir (<code>orders/dry/place</code>) yoksa bakiye/dolum satırı
                          görünmez — motor verisiyle karıştırma.
                        </li>
                      </ul>
                      <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
                        <input
                          type="checkbox"
                          checked={showDbTradingRangeLayer}
                          onChange={(e) => setShowDbTradingRangeLayer(e.target.checked)}
                        />
                        <span>Aktif grafik sembolü ile eşleşen DB Trading Range (üst / alt / orta çizgi)</span>
                      </label>
                      <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
                        <input
                          type="checkbox"
                          checked={showDbSweepMarkers}
                          onChange={(e) => setShowDbSweepMarkers(e.target.checked)}
                        />
                        <span>Son mumda DB sweep işareti (L sweep / S sweep)</span>
                      </label>
                      <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
                        <input
                          type="checkbox"
                          checked={showDbRangeSignalMarkers}
                          onChange={(e) => setShowDbRangeSignalMarkers(e.target.checked)}
                        />
                        <span>
                          DB range sinyal olayları (L/S giriş-çıkış — <code>durum</code> kenarı, F2)
                        </span>
                      </label>
                      <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
                        <input
                          type="checkbox"
                          checked={showDbOpenPositionLine}
                          onChange={(e) => setShowDbOpenPositionLine(e.target.checked)}
                        />
                        <span>
                          DB’den türetilen açık pozisyon giriş çizgisi (<code>range_signal_events</code> zinciri)
                        </span>
                      </label>
                      <button
                        type="button"
                        className="theme-toggle"
                        style={{ marginTop: "0.35rem", fontSize: "0.78rem" }}
                        disabled={engineListRefreshing}
                        onClick={async () => {
                          setEngineListRefreshing(true);
                          try {
                            await refreshEnginePanel();
                          } finally {
                            setEngineListRefreshing(false);
                          }
                        }}
                      >
                        {engineListRefreshing ? "Yenileniyor…" : "Snapshot’ları şimdi yenile"}
                      </button>
                      {enginePanelErr ? <p className="err">{enginePanelErr}</p> : null}
                      {token ? (
                        <>
                          <p className="muted" style={{ marginTop: "0.45rem", fontSize: "0.8rem" }}>
                            Hedef ekle — exchange/segment varsayılan: üst çubuk (binance / spot veya futures).
                            {rbacIsOps ? null : (
                              <span>
                                {" "}
                                <strong>Yazma</strong> (ekle / politika / motor aç-kapa) yalnızca{" "}
                                <code>trader</code> veya <code>admin</code>.
                              </span>
                            )}
                          </p>
                          {rbacIsOps ? (
                            <>
                              <div className="tv-settings__fields" style={{ marginTop: "0.35rem" }}>
                                <label>
                                  <span className="muted">symbol</span>
                                  <input
                                    className="mono"
                                    value={engineFormSymbol}
                                    onChange={(e) => setEngineFormSymbol(e.target.value)}
                                    placeholder="BTCUSDT"
                                  />
                                </label>
                                <label>
                                  <span className="muted">interval</span>
                                  <input
                                    className="mono"
                                    value={engineFormInterval}
                                    onChange={(e) => setEngineFormInterval(e.target.value)}
                                    placeholder="4h"
                                  />
                                </label>
                              </div>
                              <button
                                type="button"
                                className="theme-toggle"
                                style={{ marginTop: "0.4rem" }}
                                disabled={engineFormBusy}
                                onClick={async () => {
                                  if (!token) return;
                                  setEngineFormBusy(true);
                                  try {
                                    await postEngineSymbol(token, {
                                      symbol: engineFormSymbol.trim(),
                                      interval: engineFormInterval.trim(),
                                      exchange: barExchange.trim() || undefined,
                                      segment: barSegment.trim() || undefined,
                                    });
                                    await refreshEnginePanel();
                                  } catch (e) {
                                    setEnginePanelErr(String(e));
                                  } finally {
                                    setEngineFormBusy(false);
                                  }
                                }}
                              >
                                {engineFormBusy ? "Kayıt…" : "engine_symbols’a ekle"}
                              </button>
                            </>
                          ) : null}
                          <p className="tv-drawer__section-head" style={{ marginTop: "0.75rem" }}>
                            Kayıtlı hedefler ({engineSymbols.length})
                          </p>
                          <ul className="muted mono" style={{ fontSize: "0.72rem", maxHeight: "8rem", overflow: "auto", listStyle: "none", paddingLeft: 0 }}>
                            {engineSymbols.map((s) => (
                              <li
                                key={s.id}
                                style={{ display: "flex", alignItems: "center", gap: "0.35rem", flexWrap: "wrap", marginBottom: "0.25rem" }}
                              >
                                <span>
                                  {s.enabled ? "●" : "○"} {s.exchange}/{s.segment} {s.symbol} {s.interval}
                                  {s.label ? ` — ${s.label}` : ""}
                                </span>
                                {rbacIsOps ? (
                                  <>
                                    <select
                                      className="mono"
                                      style={{ fontSize: "0.65rem", maxWidth: "11rem" }}
                                      value={(s.signal_direction_mode ?? "auto_segment").toLowerCase()}
                                      title="Range sinyali yön politikası — spot’ta varsayılan tek yön (long), vadelide çift yön"
                                      onChange={async (e) => {
                                        if (!token) return;
                                        try {
                                          await patchEngineSymbol(token, s.id, {
                                            signal_direction_mode: e.target.value,
                                          });
                                          await refreshEnginePanel();
                                        } catch (err) {
                                          setEnginePanelErr(String(err));
                                        }
                                      }}
                                    >
                                      <option value="auto_segment">auto (segment)</option>
                                      <option value="long_only">tek yön (long)</option>
                                      <option value="both">çift yön</option>
                                      <option value="short_only">yalnız short</option>
                                    </select>
                                    <button
                                      type="button"
                                      className="theme-toggle"
                                      style={{ fontSize: "0.65rem", padding: "0.12rem 0.4rem" }}
                                      onClick={async () => {
                                        if (!token) return;
                                        try {
                                          await patchEngineSymbol(token, s.id, { enabled: !s.enabled });
                                          await refreshEnginePanel();
                                        } catch (e) {
                                          setEnginePanelErr(String(e));
                                        }
                                      }}
                                    >
                                      {s.enabled ? "Motor kapat" : "Motor aç"}
                                    </button>
                                  </>
                                ) : null}
                              </li>
                            ))}
                          </ul>
                          {matchesSetting("sinyal", "dashboard", "durum", "trend", "kopu", "range") &&
                          dbSignalDashboardSnapshot ? (
                            <div className="card" style={{ marginTop: "0.65rem", padding: "0.55rem" }}>
                              <p className="tv-drawer__section-head" style={{ marginBottom: "0.35rem" }}>
                                Sinyal paneli (DB — aktif grafik)
                              </p>
                              {dbSignalDashboardSnapshot.error ? (
                                <p className="err" style={{ fontSize: "0.75rem" }}>
                                  {dbSignalDashboardSnapshot.error}
                                </p>
                              ) : null}
                              {(() => {
                                const raw = dbSignalDashboardSnapshot.payload;
                                if (!raw || typeof raw !== "object") {
                                  return <p className="muted" style={{ fontSize: "0.75rem" }}>Payload yok</p>;
                                }
                                const ins = raw as Record<string, unknown>;
                                if (ins.reason === "insufficient_bars") {
                                  return (
                                    <p className="muted" style={{ fontSize: "0.75rem" }}>
                                      Yetersiz mum — worker ve market_bars bekleyin.
                                    </p>
                                  );
                                }
                                const p = raw as SignalDashboardPayload;
                                const row = (label: string, v: string) => (
                                  <tr key={label}>
                                    <td className="muted" style={{ padding: "0.12rem 0.35rem 0.12rem 0", verticalAlign: "top" }}>
                                      {label}
                                    </td>
                                    <td className="mono" style={{ padding: "0.12rem 0", wordBreak: "break-all" }}>
                                      {v}
                                    </td>
                                  </tr>
                                );
                                const yn = (b: boolean | undefined) => (b ? "TESPİT EDİLDİ" : "YOK");
                                return (
                                  <table style={{ width: "100%", fontSize: "0.74rem", borderCollapse: "collapse" }}>
                                    <tbody>
                                      {row("Durum", p.durum ?? "—")}
                                      {row("Durum (ham model)", p.durum_model_raw ?? "—")}
                                      {row("Yön politikası (DB)", p.signal_direction_mode ?? "—")}
                                      {row("Yön (etkin)", p.signal_direction_effective ?? "—")}
                                      {row("Yerel trend", p.yerel_trend ?? "—")}
                                      {row("Global trend", p.global_trend ?? "—")}
                                      {row("Piyasa modu", p.piyasa_modu ?? "—")}
                                      {row("Giriş modu", p.giris_modu ?? "—")}
                                      {row("Oynaklık %", p.oynaklik_pct != null ? p.oynaklik_pct.toFixed(2) : "—")}
                                      {row("Momentum 1", p.momentum_1 ?? "—")}
                                      {row("Momentum 2", p.momentum_2 ?? "—")}
                                      {row("Giriş (gerçek)", formatDashboardNumber(p.giris_gercek ?? undefined))}
                                      {row("Stop (ilk)", formatDashboardNumber(p.stop_ilk ?? undefined))}
                                      {row("Kar al (ilk)", formatDashboardNumber(p.kar_al_ilk ?? undefined))}
                                      {row("Stop/Trail (aktif)", formatDashboardNumber(p.stop_trail_aktif ?? undefined))}
                                      {row("Kar al (dyn)", formatDashboardNumber(p.kar_al_dinamik ?? undefined))}
                                      {row("Sinyal kaynağı", p.sinyal_kaynagi ?? "—")}
                                      {row("Trend tükenmesi", yn(p.trend_tukenmesi))}
                                      {row("Yapı kayması", yn(p.yapi_kaymasi))}
                                      {row("Pozisyon gücü", p.pozisyon_gucu_10 != null ? `${p.pozisyon_gucu_10} / 10` : "—")}
                                      {row("Sistem", p.sistem_aktif ? "AKTİF" : "—")}
                                      {row("Range üst", formatDashboardNumber(p.range_high ?? undefined))}
                                      {row("Range alt", formatDashboardNumber(p.range_low ?? undefined))}
                                      {row("Range orta", formatDashboardNumber(p.range_mid ?? undefined))}
                                      {row("ATR", formatDashboardNumber(p.atr ?? undefined))}
                                      {row("Son bar", p.last_bar_open_time ?? "—")}
                                    </tbody>
                                  </table>
                                );
                              })()}
                            </div>
                          ) : null}
                          {matchesSetting("paper", "dry", "f4", "ozet", "islem", "işlem", "portfolio", "birleşik") ? (
                            <div className="card" style={{ marginTop: "0.65rem", padding: "0.55rem" }}>
                              <p className="tv-drawer__section-head" style={{ marginBottom: "0.35rem" }}>
                                Range / Paper özeti (F4)
                              </p>
                              <p className="muted" style={{ fontSize: "0.72rem", marginBottom: "0.4rem" }}>
                                Üst çubuk:{" "}
                                <span className="mono">
                                  {barExchange.trim() || "—"}/{normalizeMarketSegment(barSegment)}/{barSymbol.trim() || "—"}/{barInterval.trim() || "—"}
                                </span>
                                . Canlı emirler: <code>POST /api/v1/orders/binance/place</code> — Dry:{" "}
                                <code>POST /api/v1/orders/dry/place</code>.
                              </p>
                              <table style={{ width: "100%", fontSize: "0.72rem", borderCollapse: "collapse", marginBottom: "0.45rem" }}>
                                <tbody>
                                  <tr>
                                    <td className="muted" style={{ padding: "0.1rem 0.35rem 0.1rem 0", verticalAlign: "top" }}>
                                      Motor (DB olay zinciri)
                                    </td>
                                    <td className="mono" style={{ padding: "0.1rem 0", wordBreak: "break-all" }}>
                                      {chartDerivedOpenPosition
                                        ? `${chartDerivedOpenPosition.side.toUpperCase()} @ ${chartDerivedOpenPosition.entryPrice.toFixed(4)}`
                                        : "Açık yön yok / olay yok"}
                                    </td>
                                  </tr>
                                  <tr>
                                    <td className="muted" style={{ padding: "0.1rem 0.35rem 0.1rem 0", verticalAlign: "top" }}>
                                      Son range olayları (grafik)
                                    </td>
                                    <td className="mono" style={{ padding: "0.1rem 0", fontSize: "0.68rem" }}>
                                      {chartRecentRangeEvents.length === 0 ? (
                                        "—"
                                      ) : (
                                        <span>
                                          {chartRecentRangeEvents.map((ev) => (
                                            <span key={ev.id} style={{ display: "block", marginBottom: "0.2rem" }}>
                                              {ev.event_kind} · {ev.bar_open_time}
                                              {ev.reference_price != null && Number.isFinite(ev.reference_price)
                                                ? ` · ${ev.reference_price.toFixed(4)}`
                                                : ""}
                                            </span>
                                          ))}
                                        </span>
                                      )}
                                    </td>
                                  </tr>
                                  <tr>
                                    <td className="muted" style={{ padding: "0.1rem 0.35rem 0.1rem 0", verticalAlign: "top" }}>
                                      Paper quote
                                    </td>
                                    <td className="mono" style={{ padding: "0.1rem 0" }}>
                                      {paperBalance ? String(paperBalance.quote_balance) : "Henüz dolum yok (satır oluşunca görünür)"}
                                    </td>
                                  </tr>
                                  <tr>
                                    <td className="muted" style={{ padding: "0.1rem 0.35rem 0.1rem 0", verticalAlign: "top" }}>
                                      Paper taban
                                    </td>
                                    <td className="mono" style={{ padding: "0.1rem 0", fontSize: "0.65rem", wordBreak: "break-all" }}>
                                      {paperBalance && Object.keys(paperBalance.base_positions).length > 0
                                        ? Object.entries(paperBalance.base_positions)
                                            .map(([k, v]) => `${k}: ${String(v)}`)
                                            .join(" · ")
                                        : "—"}
                                    </td>
                                  </tr>
                                </tbody>
                              </table>
                              <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.25rem" }}>
                                Son paper dolumlar (API <code>orders/dry/fills</code>)
                              </p>
                              <div style={{ maxHeight: "6.5rem", overflow: "auto", fontSize: "0.65rem" }} className="mono muted">
                                {paperFills.length === 0 ? (
                                  <span>—</span>
                                ) : (
                                  paperFills.slice(0, 6).map((f) => (
                                    <div key={f.id} style={{ marginBottom: "0.25rem" }}>
                                      {f.side} {f.exchange}/{f.segment} {f.symbol} qty {String(f.quantity)} @ {String(f.avg_price)} fee{" "}
                                      {String(f.fee)}
                                      <br />
                                      <span style={{ opacity: 0.85 }}>{f.created_at}</span>
                                    </div>
                                  ))
                                )}
                              </div>
                            </div>
                          ) : null}
                          <p className="tv-drawer__section-head" style={{ marginTop: "0.75rem" }}>
                            Snapshot özeti
                          </p>
                          <div
                            style={{ maxHeight: "10rem", overflow: "auto", fontSize: "0.72rem" }}
                            className="mono muted"
                          >
                            {engineSnapshots.length === 0 ? (
                              <span className="err">
                                Snapshot satırı yok — <code>qtss-worker</code> çalışmıyor veya henüz yazım olmadı.
                                <code>market_bars</code> + <code>DATABASE_URL</code> kontrol edin.
                              </span>
                            ) : (
                              engineSnapshots.map((s) => (
                                <div key={`${s.engine_symbol_id}-${s.engine_kind}`} style={{ marginBottom: "0.35rem" }}>
                                  <strong>{s.engine_kind}</strong> {s.symbol} {s.interval}{" "}
                                  {s.error ? <span className="err">{s.error}</span> : null}
                                  <br />
                                  {s.computed_at}
                                </div>
                              ))
                            )}
                          </div>
                          <div style={{ marginTop: "0.65rem" }}>
                            <p className="tv-drawer__section-head" style={{ marginBottom: "0.25rem" }}>
                              Range sinyal olayları (DB)
                            </p>
                            <p className="muted" style={{ fontSize: "0.7rem", marginBottom: "0.35rem" }}>
                              Worker, <code>signal_dashboard.durum</code> (LONG/SHORT/NOTR){" "}
                              <strong>önceki geçerli snapshot’a göre değişince</strong> veya ilk kez yönlü bir{" "}
                              <code>durum</code> oluşunca <code>long_entry</code> / <code>long_exit</code> /{" "}
                              <code>short_entry</code> / <code>short_exit</code> yazar. Yalnız NOTR kalıyorsa olay
                              düşmez. Mum üstü marker: F2.
                            </p>
                            <div
                              style={{ maxHeight: "9rem", overflow: "auto", fontSize: "0.68rem" }}
                              className="mono muted"
                            >
                              {engineRangeSignals.length === 0 ? (
                                <span>
                                  Henüz olay yok: worker / <code>market_bars</code> / eşleşen hedef kontrol edin; sürekli
                                  NOTR veya tick bekleniyor olabilir.
                                </span>
                              ) : (
                                engineRangeSignals.map((ev) => (
                                  <div key={ev.id} style={{ marginBottom: "0.28rem" }}>
                                    <strong>{ev.event_kind}</strong> {ev.exchange}/{ev.segment} {ev.symbol}{" "}
                                    {ev.interval}
                                    <br />
                                    bar {ev.bar_open_time}
                                    {ev.reference_price != null && Number.isFinite(ev.reference_price)
                                      ? ` · px ${ev.reference_price.toFixed(4)}`
                                      : ""}
                                  </div>
                                ))
                              )}
                            </div>
                          </div>
                        </>
                      ) : (
                        <p className="muted">Motor paneli için giriş yap.</p>
                      )}
                    </div>
                  ) : null}
                </>
              ) : null}

              {drawerTab === "nansen" ? (
                <>
                  {matchesSetting(
                    "nansen",
                    "onchain",
                    "smart",
                    "money",
                    "screener",
                    "token",
                    "api",
                    "zincir",
                  ) ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">Nansen — Token Screener (rehber)</p>
                      <p className="muted" style={{ fontSize: "0.78rem", marginBottom: "0.5rem" }}>
                        <code>qtss-worker</code> sunucuda <code>NANSEN_API_KEY</code> ile{" "}
                        <code>POST …/api/v1/token-screener</code> çağrılır; sonuç <code>nansen_snapshots</code> tablosuna
                        yazılır. Anahtar yalnızca worker ortamında tutulur — tarayıcıya veya repoya koymayın. Resmi doküman:{" "}
                        <a href="https://docs.nansen.ai/" target="_blank" rel="noreferrer">
                          docs.nansen.ai
                        </a>
                        .
                      </p>
                      <ul className="muted" style={{ fontSize: "0.72rem", margin: "0 0 0.55rem 1rem", lineHeight: 1.45 }}>
                        <li>
                          <code>NANSEN_TICK_SECS</code> — çağrı aralığı (varsayılan 600 sn); kredi ve rate limit için yüksek
                          tutun.
                        </li>
                        <li>
                          <code>NANSEN_TOKEN_SCREENER_REQUEST_JSON</code> — isteğe bağlı tam JSON gövde; yoksa Smart Money
                          + yaş filtresi + <code>buy_volume DESC</code> varsayılanı kullanılır.
                        </li>
                        <li>
                          <code>NANSEN_API_BASE</code> — varsayılan <code>https://api.nansen.ai</code>.
                        </li>
                        <li>
                          API: <code>GET …/analysis/nansen/snapshot</code> ve{" "}
                          <code>GET …/analysis/nansen/setups/latest</code> (JWT) — aşağıda özet + setup tablosu.
                        </li>
                        <li>
                          <code>QTSS_SETUP_SCAN_SECS</code> — setup tarama aralığı (varsayılan 900 sn);{" "}
                          <code>QTSS_SETUP_MAX_SNAPSHOT_AGE_SECS</code> — kullanılacak screener snapshot yaş sınırı.
                        </li>
                        <li>
                          Nansen <strong>403 Insufficient credits</strong>: hesapta kredi biter; aralığı artırın (
                          <code>NANSEN_TICK_SECS</code>) veya Nansen planını güncelleyin — snapshot satırındaki{" "}
                          <code>hata</code> API yanıtıdır.
                        </li>
                        <li>
                          <strong>404</strong> on <code>…/nansen/setups/latest</code>: çoğunlukla sunucudaki{" "}
                          <code>qtss-api</code> eski sürüm (bu uç yok). Yeni binary + restart; ayrıca{" "}
                          <code>VITE_API_BASE</code> içine <code>/api/v1</code> eklemeyin (yol çiftlenir).
                        </li>
                      </ul>
                      {token ? (
                        <>
                          <button
                            type="button"
                            className="theme-toggle"
                            style={{ fontSize: "0.78rem" }}
                            disabled={nansenRefreshing}
                            onClick={async () => {
                              setNansenRefreshing(true);
                              try {
                                await refreshNansenPanel();
                              } finally {
                                setNansenRefreshing(false);
                              }
                            }}
                          >
                            {nansenRefreshing ? "Yenileniyor…" : "Snapshot’ı şimdi yenile"}
                          </button>
                          {nansenPanelErr ? <p className="err">{nansenPanelErr}</p> : null}
                          {!nansenSnapshot ? (
                            <p className="muted" style={{ marginTop: "0.5rem" }}>
                              Henüz satır yok: migration <code>0019_nansen_snapshots</code>, worker restart ve geçerli{" "}
                              <code>NANSEN_API_KEY</code> gerekir.
                            </p>
                          ) : null}
                          <p className="tv-drawer__section-head" style={{ marginTop: "0.85rem" }}>
                            Setup taraması (son koşu)
                          </p>
                          <p className="muted" style={{ fontSize: "0.72rem", marginBottom: "0.35rem" }}>
                            Worker <code>setup_scan_engine</code> çıktısı: <code>nansen_setup_runs</code> /{" "}
                            <code>nansen_setup_rows</code> (migration <code>0020</code>). En iyi{" "}
                            <strong>5 LONG</strong> + <strong>5 SHORT</strong> (ayrı sıralı; toplam en fazla 10 satır).
                          </p>
                          {nansenSetupsLatest.setup_endpoint_missing ? (
                            <p className="err" style={{ marginTop: "0.25rem", fontSize: "0.75rem", lineHeight: 1.45 }}>
                              Setup API yanıtı <strong>404</strong>: bu yol sunucuda yok.{" "}
                              <code>qtss-api</code>’yi güncel kodla derleyip yeniden başlatın (
                              <code className="mono">GET /api/v1/analysis/nansen/setups/latest</code>). Web’de{" "}
                              <code>VITE_API_BASE</code> yalnız kök olmalı (ör. <code>http://127.0.0.1:8080</code>);{" "}
                              sonuna <code>/api/v1</code> eklemeyin.
                            </p>
                          ) : !nansenSetupsLatest.run && nansenSetupsLatest.rows.length === 0 ? (
                            <p className="muted" style={{ marginTop: "0.25rem" }}>
                              Henüz setup satırı yok: migration <code>0020</code>, worker, başarılı snapshot ve yeterli
                              Nansen kredisi gerekir.
                            </p>
                          ) : null}
                          {nansenSetupsLatest.run ? (
                            <>
                              <p className="muted mono" style={{ fontSize: "0.7rem", margin: "0.25rem 0" }}>
                                run {nansenSetupsLatest.run.computed_at} · kaynak {nansenSetupsLatest.run.source} · aday{" "}
                                {nansenSetupsLatest.run.candidate_count}
                                {nansenSetupsLatest.run.error ? (
                                  <>
                                    {" "}
                                    · <span className="err">{nansenSetupsLatest.run.error}</span>
                                  </>
                                ) : null}
                              </p>
                              {nansenSetupsLatest.run.meta_json != null &&
                              typeof nansenSetupsLatest.run.meta_json === "object" ? (
                                <pre
                                  className="mono muted"
                                  style={{
                                    maxHeight: "4.5rem",
                                    overflow: "auto",
                                    fontSize: "0.62rem",
                                    margin: "0 0 0.4rem 0",
                                  }}
                                >
                                  {JSON.stringify(nansenSetupsLatest.run.meta_json, null, 2)}
                                </pre>
                              ) : null}
                            </>
                          ) : null}
                          {nansenSetupsLatest.rows.length > 0 ? (
                            <div
                              style={{ maxHeight: "17rem", overflow: "auto", fontSize: "0.68rem" }}
                              className="mono muted"
                            >
                              <table
                                style={{
                                  width: "100%",
                                  borderCollapse: "collapse",
                                  fontSize: "inherit",
                                }}
                              >
                                <thead>
                                  <tr style={{ textAlign: "left", borderBottom: "1px solid var(--tv-border, #333)" }}>
                                    <th style={{ padding: "0.2rem 0.35rem 0.2rem 0" }}>#</th>
                                    <th style={{ padding: "0.2rem 0.35rem" }}>Sembol</th>
                                    <th style={{ padding: "0.2rem 0.35rem" }}>Yön</th>
                                    <th style={{ padding: "0.2rem 0.35rem" }}>Skor</th>
                                    <th style={{ padding: "0.2rem 0.35rem" }}>p</th>
                                    <th style={{ padding: "0.2rem 0.35rem" }}>RR</th>
                                    <th style={{ padding: "0.2rem 0.35rem" }}>Giriş</th>
                                    <th style={{ padding: "0.2rem 0.35rem" }}>SL</th>
                                    <th style={{ padding: "0.2rem 0.35rem" }}>TP1</th>
                                    <th style={{ padding: "0.2rem 0.35rem" }}>TP2</th>
                                    <th style={{ padding: "0.2rem 0.35rem" }}>TP3</th>
                                    <th style={{ padding: "0.2rem 0.35rem" }} title="Fiyata göre TP2 mesafesi %">
                                      Δ%2
                                    </th>
                                  </tr>
                                </thead>
                                <tbody>
                                  {nansenSetupsLatest.rows.map((row) => (
                                    <tr key={row.id} style={{ borderBottom: "1px solid var(--tv-border, #222)" }}>
                                      <td style={{ padding: "0.25rem 0.35rem 0.25rem 0" }}>{row.rank}</td>
                                      <td style={{ padding: "0.25rem 0.35rem" }}>
                                        <strong>{row.token_symbol}</strong>
                                        <span className="muted"> · {row.chain}</span>
                                      </td>
                                      <td style={{ padding: "0.25rem 0.35rem" }}>{row.direction}</td>
                                      <td style={{ padding: "0.25rem 0.35rem" }}>{row.score}</td>
                                      <td style={{ padding: "0.25rem 0.35rem" }}>{row.probability.toFixed(2)}</td>
                                      <td style={{ padding: "0.25rem 0.35rem" }}>{row.rr.toFixed(2)}</td>
                                      <td style={{ padding: "0.25rem 0.35rem" }}>{row.entry.toPrecision(4)}</td>
                                      <td style={{ padding: "0.25rem 0.35rem" }}>{row.stop_loss.toPrecision(4)}</td>
                                      <td style={{ padding: "0.25rem 0.35rem" }}>{row.tp1.toPrecision(4)}</td>
                                      <td style={{ padding: "0.25rem 0.35rem" }}>{row.tp2.toPrecision(4)}</td>
                                      <td style={{ padding: "0.25rem 0.35rem" }}>{row.tp3.toPrecision(4)}</td>
                                      <td style={{ padding: "0.25rem 0.35rem" }}>
                                        {Number.isFinite(row.pct_to_tp2)
                                          ? `${row.pct_to_tp2.toFixed(1)}%`
                                          : "—"}
                                      </td>
                                    </tr>
                                  ))}
                                </tbody>
                              </table>
                              <div style={{ marginTop: "0.45rem", lineHeight: 1.35 }}>
                                {nansenSetupsLatest.rows.map((row) => (
                                  <details key={`${row.id}-sig`} style={{ marginBottom: "0.35rem" }}>
                                    <summary style={{ cursor: "pointer" }}>
                                      #{row.rank} {row.token_symbol} — {row.setup}
                                      {row.ohlc_enriched ? "" : " · OHLC eksik"}
                                    </summary>
                                    <ul style={{ margin: "0.25rem 0 0 1rem", padding: 0 }}>
                                      {Array.isArray(row.key_signals)
                                        ? (row.key_signals as unknown[]).map((s, i) => (
                                            <li key={i}>{String(s)}</li>
                                          ))
                                        : (
                                            <li className="muted">(sinyal listesi yok)</li>
                                          )}
                                    </ul>
                                  </details>
                                ))}
                              </div>
                            </div>
                          ) : null}
                          {nansenSnapshot ? (
                            <div style={{ marginTop: "0.55rem" }}>
                              <p className="muted mono" style={{ fontSize: "0.72rem", margin: "0 0 0.25rem 0" }}>
                                computed_at {nansenSnapshot.computed_at}
                                {nansenSnapshot.error ? (
                                  <>
                                    {" "}
                                    · <span className="err">hata: {nansenSnapshot.error}</span>
                                  </>
                                ) : null}
                              </p>
                              <p className="muted" style={{ fontSize: "0.7rem", margin: "0 0 0.2rem 0" }}>
                                meta (kredi / limit özeti)
                              </p>
                              <pre
                                className="mono"
                                style={{ maxHeight: "6rem", overflow: "auto", fontSize: "0.68rem", marginBottom: "0.45rem" }}
                              >
                                {JSON.stringify(nansenSnapshot.meta_json ?? {}, null, 2)}
                              </pre>
                              <p className="muted" style={{ fontSize: "0.7rem", margin: "0 0 0.2rem 0" }}>
                                request_json
                              </p>
                              <pre
                                className="mono"
                                style={{ maxHeight: "8rem", overflow: "auto", fontSize: "0.68rem", marginBottom: "0.45rem" }}
                              >
                                {JSON.stringify(nansenSnapshot.request_json ?? {}, null, 2)}
                              </pre>
                              <p className="muted" style={{ fontSize: "0.7rem", margin: "0 0 0.2rem 0" }}>
                                response_json
                              </p>
                              <pre
                                className="mono"
                                style={{ maxHeight: "16rem", overflow: "auto", fontSize: "0.65rem" }}
                              >
                                {nansenSnapshot.response_json != null
                                  ? JSON.stringify(nansenSnapshot.response_json, null, 2)
                                  : "—"}
                              </pre>
                            </div>
                          ) : null}
                        </>
                      ) : (
                        <p className="muted">Nansen snapshot için giriş yapın.</p>
                      )}
                    </div>
                  ) : null}
                </>
              ) : null}

              {drawerTab === "setting" ? (
                <>
                  {token && rbacIsOps && matchesSetting("market bars", "backfill", "exchange", "segment", "limit") ? (
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
