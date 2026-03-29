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
  type DataSnapshotApiRow,
  type ExternalDataSourceApiRow,
  fetchEngineSnapshots,
  fetchDataSnapshots,
  fetchExternalFetchSources,
  fetchMarketContextLatest,
  fetchMarketContextSummary,
  fetchOnchainSignalsBreakdown,
  fetchConfluenceSnapshotsLatest,
  fetchEngineRangeSignals,
  fetchEngineSymbols,
  fetchNansenSnapshot,
  fetchNansenSetupsLatest,
  fetchPaperBalance,
  fetchPaperFills,
  fetchBinanceCommissionDefaults,
  fetchBinanceCommissionAccount,
  postEngineSymbol,
  patchEngineSymbol,
  type EngineSnapshotJoinedApiRow,
  type EngineSymbolApiRow,
  type MarketContextLatestApiResponse,
  type MarketContextSummaryItemApi,
  type NansenSetupsLatestApiResponse,
  type NansenSnapshotApiRow,
  type PaperBalanceRow,
  type PaperFillRow,
  type BinanceCommissionDefaultsApi,
  type BinanceCommissionAccountApi,
  type RangeSignalEventApiRow,
  type OnchainSignalsBreakdownApi,
} from "./api/client";
import { channelDrawingToOverlay } from "./lib/channelOverlayFromDrawing";
import { buildChannelScanPivotMarkers } from "./lib/channelScanMarkers";
import {
  buildMultiPatternOverlayFromScan,
  type PatternLayerOverlay,
  type MultiPatternChartOverlay,
} from "./lib/patternDrawingBatchOverlay";
import { ChannelScanMatchesTable } from "./components/ChannelScanMatchesTable";
import { OperationsQueuesPanel } from "./components/OperationsQueuesPanel";
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
import {
  formatDashboardNumber,
  parseSignalDashboardV2,
  pickDashboardBool,
  pickDashboardNum,
  pickDashboardStr,
  type SignalDashboardPayload,
} from "./lib/signalDashboardPayload";
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
  | "market_context"
  | "nansen"
  | "queues"
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

/** PLAN §4.2: confluence `lot_scale_hint`, `data_sources_considered`, `conflicts[].code` (wire English keys). */
function formatConfluenceExtras(p: Record<string, unknown>): string {
  const parts: string[] = [];
  if (typeof p.lot_scale_hint === "number" && Number.isFinite(p.lot_scale_hint)) {
    parts.push(`lot_scale ${p.lot_scale_hint.toFixed(2)}`);
  }
  const dsc = p.data_sources_considered;
  if (Array.isArray(dsc) && dsc.length > 0 && dsc.every((x) => typeof x === "string")) {
    parts.push(`sources ${(dsc as string[]).join(", ")}`);
  }
  const raw = p.conflicts;
  if (Array.isArray(raw)) {
    const codes = raw
      .map((x) =>
        x && typeof x === "object" && typeof (x as Record<string, unknown>).code === "string"
          ? String((x as Record<string, unknown>).code)
          : null,
      )
      .filter((c): c is string => Boolean(c));
    const n = codes.length;
    if (n === 0) {
      parts.push("conflicts 0");
    } else {
      const preview = codes.slice(0, 3).join(", ");
      parts.push(n > 3 ? `conflicts ${n}: ${preview}…` : `conflicts ${n}: ${preview}`);
    }
  }
  return parts.length ? ` · ${parts.join(" · ")}` : "";
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

  const runChannelSixScanWithBars = useCallback(
    async (barsInput: ChartOhlcRow[]) => {
      if (!token || !barsInput.length) return;
      setChannelScanError("");
      setChannelScanJson("");
      setChannelScanLoading(true);
      try {
        const scanWindow = acpOhlcWindowForScan(
          barsInput,
          acpConfig.calculated_bars,
          acpConfig.scanning.repaint,
        );
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
            res.pattern_matches?.map((m) => m.pattern_name ?? `id ${m.outcome.scan.pattern_type_id}`).join(" · ") ??
            name;
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
    },
    [token, acpConfig, theme],
  );

  const runChannelSixScan = useCallback(() => {
    if (!bars?.length) return;
    void runChannelSixScanWithBars(bars);
  }, [bars, runChannelSixScanWithBars]);

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
  const [dataSnapshots, setDataSnapshots] = useState<DataSnapshotApiRow[]>([]);
  const [marketContext, setMarketContext] = useState<MarketContextLatestApiResponse | null>(null);
  const [contextTabSingle, setContextTabSingle] = useState<MarketContextLatestApiResponse | null>(null);
  const [contextTabSummaries, setContextTabSummaries] = useState<MarketContextSummaryItemApi[]>([]);
  const [contextTabConfluence, setContextTabConfluence] = useState<EngineSnapshotJoinedApiRow[]>([]);
  const [contextTabConfluenceEndpointMissing, setContextTabConfluenceEndpointMissing] = useState(false);
  const [contextTabDataSnaps, setContextTabDataSnaps] = useState<DataSnapshotApiRow[]>([]);
  const [contextTabExternalSources, setContextTabExternalSources] = useState<ExternalDataSourceApiRow[]>([]);
  const [contextTabOnchainBreakdown, setContextTabOnchainBreakdown] = useState<OnchainSignalsBreakdownApi | null>(
    null,
  );
  const [contextTabOnchainEndpointMissing, setContextTabOnchainEndpointMissing] = useState(false);
  const [contextTabErr, setContextTabErr] = useState("");
  const [contextTabBusy, setContextTabBusy] = useState(false);
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
  const [commissionDefaults, setCommissionDefaults] = useState<BinanceCommissionDefaultsApi | null>(null);
  const [commissionAccount, setCommissionAccount] = useState<BinanceCommissionAccountApi | null>(null);
  const [commissionAccountErr, setCommissionAccountErr] = useState("");
  const [commissionAccountBusy, setCommissionAccountBusy] = useState(false);
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
      const symQ = barSymbol.trim().toUpperCase();
      const mcPromise =
        symQ.length > 0
          ? fetchMarketContextLatest(token, {
              symbol: symQ,
              interval: barInterval.trim(),
              exchange: barExchange.trim().toLowerCase(),
              segment: (barSegment.trim() || "spot").toLowerCase(),
            }).catch(() => null)
          : Promise.resolve(null);
      const segLower = (barSegment.trim() || "spot").toLowerCase();
      const commDefPromise = fetchBinanceCommissionDefaults(token, {
        segment: segLower,
        symbol: symQ.length > 0 ? symQ : undefined,
      }).catch(() => null);
      const [snaps, syms, sigs, pbal, pfills, ds, mc, commDef] = await Promise.all([
        fetchEngineSnapshots(token),
        fetchEngineSymbols(token),
        fetchEngineRangeSignals(token, { limit: 80 }),
        fetchPaperBalance(token).catch(() => null),
        fetchPaperFills(token, 15).catch(() => []),
        fetchDataSnapshots(token).catch(() => []),
        mcPromise,
        commDefPromise,
      ]);
      setEngineSnapshots(snaps);
      setDataSnapshots(ds);
      setMarketContext(mc);
      setEngineSymbols(syms);
      setEngineRangeSignals(sigs);
      setPaperBalance(pbal);
      setPaperFills(pfills);
      setCommissionDefaults(commDef);
      setCommissionAccount(null);
      setCommissionAccountErr("");
      setEnginePanelErr("");
    } catch (e) {
      setEnginePanelErr(String(e));
    }
  }, [token, barSymbol, barInterval, barExchange, barSegment]);

  const loadCommissionAccount = useCallback(async () => {
    if (!token) return;
    const sym = barSymbol.trim().toUpperCase();
    if (!sym) {
      setCommissionAccountErr("Üst çubukta sembol gerekli.");
      return;
    }
    setCommissionAccountBusy(true);
    setCommissionAccountErr("");
    try {
      const r = await fetchBinanceCommissionAccount(token, {
        symbol: sym,
        segment: (barSegment.trim() || "spot").toLowerCase(),
      });
      setCommissionAccount(r);
    } catch (e) {
      setCommissionAccount(null);
      setCommissionAccountErr(String(e));
    } finally {
      setCommissionAccountBusy(false);
    }
  }, [token, barSymbol, barSegment]);

  const refreshMarketContextPanel = useCallback(async () => {
    if (!token) return;
    setContextTabBusy(true);
    setContextTabErr("");
    try {
      const symQ = barSymbol.trim().toUpperCase();
      const sumQ: {
        enabled_only: boolean;
        limit: number;
        exchange?: string;
        segment?: string;
        symbol?: string;
      } = { enabled_only: true, limit: symQ ? 80 : 200 };
      if (symQ) {
        sumQ.symbol = symQ;
        const ex = barExchange.trim().toLowerCase();
        if (ex) sumQ.exchange = ex;
        sumQ.segment = (barSegment.trim() || "spot").toLowerCase();
      }
      const [cfOut, ds, summaries, extSrc, onchainPart] = await Promise.all([
        fetchConfluenceSnapshotsLatest(token),
        fetchDataSnapshots(token).catch(() => []),
        fetchMarketContextSummary(token, sumQ).catch(() => []),
        fetchExternalFetchSources(token).catch(() => []),
        symQ.length > 0
          ? fetchOnchainSignalsBreakdown(token, symQ).catch(() => ({
              data: null,
              endpoint_missing: false,
            }))
          : Promise.resolve({ data: null, endpoint_missing: false }),
      ]);
      setContextTabConfluence(cfOut.rows);
      setContextTabConfluenceEndpointMissing(Boolean(cfOut.endpoint_missing));
      setContextTabDataSnaps(ds);
      setContextTabExternalSources(extSrc);
      setContextTabSummaries(summaries);
      if (symQ.length > 0) {
        setContextTabOnchainBreakdown(onchainPart.data);
        setContextTabOnchainEndpointMissing(onchainPart.endpoint_missing);
        const one = await fetchMarketContextLatest(token, {
          symbol: symQ,
          interval: barInterval.trim(),
          exchange: barExchange.trim().toLowerCase(),
          segment: (barSegment.trim() || "spot").toLowerCase(),
        }).catch(() => null);
        setContextTabSingle(one);
      } else {
        setContextTabOnchainBreakdown(null);
        setContextTabOnchainEndpointMissing(false);
        setContextTabSingle(null);
      }
    } catch (e) {
      setContextTabErr(String(e));
      setContextTabConfluence([]);
      setContextTabConfluenceEndpointMissing(false);
      setContextTabDataSnaps([]);
      setContextTabExternalSources([]);
      setContextTabSummaries([]);
      setContextTabOnchainBreakdown(null);
      setContextTabOnchainEndpointMissing(false);
      setContextTabSingle(null);
    } finally {
      setContextTabBusy(false);
    }
  }, [token, barSymbol, barInterval, barExchange, barSegment]);

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
      setDataSnapshots([]);
      setMarketContext(null);
      setContextTabSingle(null);
      setContextTabSummaries([]);
      setContextTabConfluence([]);
      setContextTabConfluenceEndpointMissing(false);
      setContextTabDataSnaps([]);
      setContextTabExternalSources([]);
      setContextTabOnchainBreakdown(null);
      setContextTabOnchainEndpointMissing(false);
      setContextTabErr("");
      setEngineRangeSignals([]);
      setEngineSymbols([]);
      setPaperBalance(null);
      setPaperFills([]);
      setCommissionDefaults(null);
      setCommissionAccount(null);
      setCommissionAccountErr("");
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

  useEffect(() => {
    if (!drawerOpen || drawerTab !== "market_context" || !token) return;
    void refreshMarketContextPanel();
    const id = window.setInterval(() => {
      void refreshMarketContextPanel();
    }, 90_000);
    return () => window.clearInterval(id);
  }, [drawerOpen, drawerTab, token, refreshMarketContextPanel]);

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
        if (acpConfig.scanning.auto_scan_on_timeframe_change) {
          void runChannelSixScanWithBars(rows);
        }
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
        if (acpConfig.scanning.auto_scan_on_timeframe_change) {
          void runChannelSixScanWithBars(rows);
        }
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
  }, [
    token,
    barExchange,
    barSegment,
    barSymbol,
    barInterval,
    barLimit,
    clearChannelScanUi,
    ohlcFromBinance,
    acpConfig.scanning.auto_scan_on_timeframe_change,
    runChannelSixScanWithBars,
  ]);

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

  /** ACP’de otomatik tarama kapalıyken açılırsa (grafik zaten yüklü) bir kez tara. */
  const prevAutoScanRef = useRef<boolean | null>(null);
  useEffect(() => {
    const on = acpConfig.scanning.auto_scan_on_timeframe_change;
    const prev = prevAutoScanRef.current;
    if (prev === false && on && token && bars?.length) {
      void runChannelSixScan();
    }
    prevAutoScanRef.current = on;
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
                  placeholder="Ayar ara (örn. zigzag, bağlam, confluence, paper, komisyon)"
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
                  aria-selected={drawerTab === "market_context"}
                  className={`tv-settings__tab ${drawerTab === "market_context" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("market_context")}
                >
                  Bağlam
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
                  aria-selected={drawerTab === "queues"}
                  className={`tv-settings__tab ${drawerTab === "queues" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("queues")}
                >
                  Kuyruklar
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
                          <br />
                          permissions=
                          {authSession.permissions.length ? authSession.permissions.join(", ") : "—"}
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
                        ACP ayarlarında <strong>Timeframe değişince otomatik kanal taraması</strong> açıkken: grafik
                        mumları her yüklendiğinde (sembol / kaynak / limit yenileme) ve üst şerit interval
                        değiştiğinde tarama çalışır; seçeneği sonradan açarsanız bir kez daha tetiklenir.
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
                        <li>
                          <strong>Confluence (F7):</strong> Worker <code>signal_dashboard</code> sonrası{" "}
                          <code>engine_kind = confluence</code> yazar; ham HTTP/Nansen birleşimi{" "}
                          <code>data_snapshots</code> içinde. <code>QTSS_CONFLUENCE_ENGINE</code> ile kapatılabilir.
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
                          {matchesSetting(
                            "sinyal",
                            "dashboard",
                            "durum",
                            "trend",
                            "kopu",
                            "range",
                            "v2",
                            "ingilizce",
                            "signal_dashboard",
                            "wire",
                          ) &&
                          dbSignalDashboardSnapshot ? (
                            <div className="card" style={{ marginTop: "0.65rem", padding: "0.55rem" }}>
                              <p className="tv-drawer__section-head" style={{ marginBottom: "0.35rem" }}>
                                Sinyal paneli (DB — aktif grafik)
                              </p>
                              <p className="muted" style={{ fontSize: "0.66rem", marginBottom: "0.35rem" }}>
                                Öncelik: <code>signal_dashboard_v2</code> (İngilizce wire, <code>schema_version</code> 3); yoksa
                                Türkçe v1 alanları.
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
                                const v2 = parseSignalDashboardV2(ins.signal_dashboard_v2);
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
                                const posStr =
                                  v2?.position_strength_10 != null
                                    ? `${v2.position_strength_10} / 10`
                                    : p.pozisyon_gucu_10 != null
                                      ? `${p.pozisyon_gucu_10} / 10`
                                      : "—";
                                const sysStr =
                                  pickDashboardBool(v2?.system_active, p.sistem_aktif) === true ? "AKTİF" : "—";
                                const wireRow = (key: string, val: unknown) => {
                                  if (val === undefined || val === null) return null;
                                  const s = typeof val === "boolean" ? (val ? "true" : "false") : String(val);
                                  return (
                                    <tr key={key}>
                                      <td
                                        className="muted mono"
                                        style={{ padding: "0.08rem 0.35rem 0.08rem 0", verticalAlign: "top", width: "42%" }}
                                      >
                                        {key}
                                      </td>
                                      <td className="mono" style={{ padding: "0.08rem 0", wordBreak: "break-all" }}>
                                        {s}
                                      </td>
                                    </tr>
                                  );
                                };
                                return (
                                  <>
                                  <table style={{ width: "100%", fontSize: "0.74rem", borderCollapse: "collapse" }}>
                                    <tbody>
                                      {row("Durum", pickDashboardStr(v2?.status, p.durum))}
                                      {row("Durum (ham model)", pickDashboardStr(v2?.status_model_raw, p.durum_model_raw))}
                                      {row("Yön politikası (DB)", p.signal_direction_mode ?? "—")}
                                      {row("Yön (etkin)", p.signal_direction_effective ?? "—")}
                                      {row("Yerel trend", pickDashboardStr(v2?.local_trend, p.yerel_trend))}
                                      {row("Global trend", pickDashboardStr(v2?.global_trend, p.global_trend))}
                                      {row("Piyasa modu", pickDashboardStr(v2?.market_mode, p.piyasa_modu))}
                                      {row("Giriş modu", pickDashboardStr(v2?.entry_mode, p.giris_modu))}
                                      {row(
                                        "Oynaklık %",
                                        v2?.volatility_pct != null && Number.isFinite(v2.volatility_pct)
                                          ? v2.volatility_pct.toFixed(2)
                                          : p.oynaklik_pct != null
                                            ? p.oynaklik_pct.toFixed(2)
                                            : "—",
                                      )}
                                      {row("Momentum 1", pickDashboardStr(v2?.momentum_rsi, p.momentum_1))}
                                      {row("Momentum 2", pickDashboardStr(v2?.momentum_roc, p.momentum_2))}
                                      {row("Giriş (gerçek)", pickDashboardNum(v2?.entry_price ?? undefined, p.giris_gercek ?? undefined))}
                                      {row("Stop (ilk)", pickDashboardNum(v2?.stop_initial ?? undefined, p.stop_ilk ?? undefined))}
                                      {row("Kar al (ilk)", pickDashboardNum(v2?.take_profit_initial ?? undefined, p.kar_al_ilk ?? undefined))}
                                      {row(
                                        "Stop/Trail (aktif)",
                                        pickDashboardNum(v2?.stop_trail ?? undefined, p.stop_trail_aktif ?? undefined),
                                      )}
                                      {row(
                                        "Kar al (dyn)",
                                        pickDashboardNum(v2?.take_profit_dynamic ?? undefined, p.kar_al_dinamik ?? undefined),
                                      )}
                                      {row("Sinyal kaynağı", pickDashboardStr(v2?.signal_source, p.sinyal_kaynagi))}
                                      {row("Trend tükenmesi", yn(pickDashboardBool(v2?.trend_exhaustion, p.trend_tukenmesi)))}
                                      {row("Yapı kayması", yn(pickDashboardBool(v2?.structure_shift, p.yapi_kaymasi)))}
                                      {row("Pozisyon gücü", posStr)}
                                      {row("Sistem", sysStr)}
                                      {row("Range üst", formatDashboardNumber(p.range_high ?? undefined))}
                                      {row("Range alt", formatDashboardNumber(p.range_low ?? undefined))}
                                      {row("Range orta", formatDashboardNumber(p.range_mid ?? undefined))}
                                      {row("ATR", formatDashboardNumber(p.atr ?? undefined))}
                                      {row("Son bar", p.last_bar_open_time ?? "—")}
                                    </tbody>
                                  </table>
                                  {v2 ? (
                                    <details style={{ marginTop: "0.45rem" }}>
                                      <summary className="muted" style={{ fontSize: "0.7rem", cursor: "pointer" }}>
                                        Wire (EN) — <code>signal_dashboard_v2</code>
                                      </summary>
                                      <table
                                        style={{ width: "100%", fontSize: "0.68rem", borderCollapse: "collapse", marginTop: "0.28rem" }}
                                        className="mono muted"
                                      >
                                        <tbody>
                                          {wireRow("schema_version", v2.schema_version)}
                                          {wireRow("status", v2.status)}
                                          {wireRow("status_model_raw", v2.status_model_raw)}
                                          {wireRow("local_trend", v2.local_trend)}
                                          {wireRow("global_trend", v2.global_trend)}
                                          {wireRow("market_mode", v2.market_mode)}
                                          {wireRow("entry_mode", v2.entry_mode)}
                                          {wireRow("volatility_pct", v2.volatility_pct)}
                                          {wireRow("momentum_rsi", v2.momentum_rsi)}
                                          {wireRow("momentum_roc", v2.momentum_roc)}
                                          {wireRow("entry_price", v2.entry_price)}
                                          {wireRow("stop_initial", v2.stop_initial)}
                                          {wireRow("take_profit_initial", v2.take_profit_initial)}
                                          {wireRow("stop_trail", v2.stop_trail)}
                                          {wireRow("take_profit_dynamic", v2.take_profit_dynamic)}
                                          {wireRow("signal_source", v2.signal_source)}
                                          {wireRow("trend_exhaustion", v2.trend_exhaustion)}
                                          {wireRow("structure_shift", v2.structure_shift)}
                                          {wireRow("position_strength_10", v2.position_strength_10)}
                                          {wireRow("system_active", v2.system_active)}
                                        </tbody>
                                      </table>
                                    </details>
                                  ) : null}
                                  </>
                                );
                              })()}
                            </div>
                          ) : null}
                          {matchesSetting(
                            "paper",
                            "dry",
                            "f4",
                            "ozet",
                            "islem",
                            "işlem",
                            "portfolio",
                            "birleşik",
                            "komisyon",
                            "commission",
                            "fee",
                            "ücret",
                            "maker",
                            "taker",
                            "f5",
                          ) ? (
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
                              <p className="tv-drawer__section-head" style={{ marginBottom: "0.3rem", marginTop: "0.45rem" }}>
                                Komisyon özeti (F5 / SPEC §7.2)
                              </p>
                              <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.35rem" }}>
                                Varsayılan: <code>GET …/market/binance/commission-defaults</code> (motor yenilemede). Hesap:{" "}
                                <code>…/commission-account</code> — Binance API anahtarı <code>exchange_accounts</code>.
                              </p>
                              <table
                                style={{ width: "100%", fontSize: "0.72rem", borderCollapse: "collapse", marginBottom: "0.35rem" }}
                              >
                                <tbody>
                                  <tr>
                                    <td className="muted" style={{ padding: "0.1rem 0.35rem 0.1rem 0", verticalAlign: "top" }}>
                                      Varsayılan (bps)
                                    </td>
                                    <td className="mono" style={{ padding: "0.1rem 0", wordBreak: "break-all" }}>
                                      {commissionDefaults ? (
                                        <>
                                          maker {commissionDefaults.defaults_bps.maker_bps.toFixed(2)} · taker{" "}
                                          {commissionDefaults.defaults_bps.taker_bps.toFixed(2)}
                                          <br />
                                          <span style={{ opacity: 0.88 }}>
                                            {commissionDefaults.segment}
                                            {commissionDefaults.query_symbol ? ` · ${commissionDefaults.query_symbol}` : ""} ·{" "}
                                            {commissionDefaults.source}
                                          </span>
                                        </>
                                      ) : (
                                        "—"
                                      )}
                                    </td>
                                  </tr>
                                  <tr>
                                    <td className="muted" style={{ padding: "0.1rem 0.35rem 0.1rem 0", verticalAlign: "top" }}>
                                      Hesap (kesir)
                                    </td>
                                    <td className="mono" style={{ padding: "0.1rem 0", wordBreak: "break-all" }}>
                                      {commissionAccount ? (
                                        <>
                                          maker {commissionAccount.maker_rate} · taker {commissionAccount.taker_rate}
                                          <br />
                                          <span style={{ opacity: 0.88 }}>
                                            {commissionAccount.segment} · {commissionAccount.source}
                                          </span>
                                        </>
                                      ) : commissionAccountErr ? (
                                        <span className="err">{commissionAccountErr}</span>
                                      ) : (
                                        <span className="muted">—</span>
                                      )}
                                    </td>
                                  </tr>
                                </tbody>
                              </table>
                              <button
                                type="button"
                                className="theme-toggle"
                                style={{ fontSize: "0.74rem", marginBottom: "0.45rem" }}
                                disabled={commissionAccountBusy}
                                onClick={() => void loadCommissionAccount()}
                              >
                                {commissionAccountBusy ? "Hesap komisyonu…" : "Hesap komisyonunu çek"}
                              </button>
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
                          <p className="tv-drawer__section-head" style={{ marginTop: "0.65rem" }}>
                            Confluence özeti
                          </p>
                          <div
                            style={{ maxHeight: "6rem", overflow: "auto", fontSize: "0.72rem" }}
                            className="mono muted"
                          >
                            {engineSnapshots.filter((s) => s.engine_kind === "confluence").length === 0 ? (
                              <span>
                                Henüz <code>confluence</code> satırı yok — worker veya{" "}
                                <code>QTSS_CONFLUENCE_ENGINE=off</code> / yetersiz bar kontrol edin.
                              </span>
                            ) : (
                              engineSnapshots
                                .filter((s) => s.engine_kind === "confluence")
                                .map((s) => {
                                  const p =
                                    s.payload && typeof s.payload === "object"
                                      ? (s.payload as Record<string, unknown>)
                                      : null;
                                  const comp =
                                    typeof p?.composite_score === "number"
                                      ? p.composite_score.toFixed(3)
                                      : "—";
                                  const reg = typeof p?.regime === "string" ? p.regime : "—";
                                  const conf =
                                    typeof p?.confidence_0_100 === "number" ? String(p.confidence_0_100) : "—";
                                  const extras = p ? formatConfluenceExtras(p) : "";
                                  return (
                                    <div key={`cf-${s.engine_symbol_id}`} style={{ marginBottom: "0.3rem" }}>
                                      {s.symbol} {s.interval} · regime {reg} · composite {comp} · conf {conf}
                                      {extras}
                                      <br />
                                      {s.computed_at}
                                      {s.error ? (
                                        <>
                                          <br />
                                          <span className="err">{s.error}</span>
                                        </>
                                      ) : null}
                                    </div>
                                  );
                                })
                            )}
                          </div>
                          <p className="tv-drawer__section-head" style={{ marginTop: "0.55rem" }}>
                            Birleşik <code>data_snapshots</code>
                          </p>
                          <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.25rem" }}>
                            Nansen + harici çekimler tek satır/kaynak; confluence buradan okur (ör.{" "}
                            <code>binance_taker_btcusdt</code>).
                          </p>
                          <div
                            style={{ maxHeight: "7rem", overflow: "auto", fontSize: "0.7rem" }}
                            className="mono muted"
                          >
                            {dataSnapshots.length === 0 ? (
                              <span>Henüz satır yok — worker yazımı veya migration 0022 kontrol edin.</span>
                            ) : (
                              dataSnapshots.map((d) => (
                                <div key={d.source_key} style={{ marginBottom: "0.28rem" }}>
                                  <strong>{d.source_key}</strong>
                                  {d.error ? <span className="err"> {d.error}</span> : null}
                                  <br />
                                  {d.computed_at}
                                </div>
                              ))
                            )}
                          </div>
                          <p className="tv-drawer__section-head" style={{ marginTop: "0.55rem" }}>
                            Piyasa bağlamı (üst çubuk)
                          </p>
                          <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.25rem" }}>
                            <code>GET …/analysis/market-context/latest</code> — tek <code>engine_symbols</code> hedefi
                            için <code>signal_dashboard</code>, <code>trading_range</code>, <code>confluence</code> ve
                            Nansen + taker <code>data_snapshots</code>.
                          </p>
                          <div
                            style={{ maxHeight: "9rem", overflow: "auto", fontSize: "0.7rem" }}
                            className="mono muted"
                          >
                            {!barSymbol.trim() ? (
                              <span>Üst çubukta sembol seçin.</span>
                            ) : !marketContext ? (
                              <span>
                                Bu exchange/segment/symbol/interval için <code>engine_symbols</code> yok veya API 404 —
                                motor hedefini ekleyin.
                              </span>
                            ) : (
                              <>
                                <div style={{ marginBottom: "0.35rem" }}>
                                  <strong>
                                    {marketContext.exchange}/{marketContext.segment} {marketContext.symbol}{" "}
                                    {marketContext.interval}
                                  </strong>
                                </div>
                                {(() => {
                                  const dash = marketContext.technical.signal_dashboard;
                                  const d =
                                    dash && typeof dash === "object"
                                      ? (dash as Record<string, unknown>)
                                      : null;
                                  const durum = typeof d?.durum === "string" ? d.durum : "—";
                                  const piyasa =
                                    typeof d?.piyasa_modu === "string" ? d.piyasa_modu : "—";
                                  return (
                                    <div style={{ marginBottom: "0.3rem" }}>
                                      TA: durum <strong>{durum}</strong> · piyasa_modu <strong>{piyasa}</strong>
                                    </div>
                                  );
                                })()}
                                {(() => {
                                  const cf = marketContext.confluence;
                                  const p =
                                    cf && typeof cf === "object" ? (cf as Record<string, unknown>) : null;
                                  if (!p) {
                                    return (
                                      <div style={{ marginBottom: "0.3rem" }}>
                                        Confluence: <span className="muted">—</span>
                                      </div>
                                    );
                                  }
                                  const comp =
                                    typeof p.composite_score === "number"
                                      ? p.composite_score.toFixed(3)
                                      : "—";
                                  const reg = typeof p.regime === "string" ? p.regime : "—";
                                  const conf =
                                    typeof p.confidence_0_100 === "number"
                                      ? String(p.confidence_0_100)
                                      : "—";
                                  const extras = formatConfluenceExtras(p);
                                  return (
                                    <div style={{ marginBottom: "0.3rem" }}>
                                      Confluence: regime <strong>{reg}</strong> · composite <strong>{comp}</strong> ·
                                      conf <strong>{conf}</strong>
                                      {extras ? (
                                        <>
                                          <br />
                                          <span style={{ opacity: 0.92 }}>{extras.replace(/^ · /, "")}</span>
                                        </>
                                      ) : null}
                                    </div>
                                  );
                                })()}
                                {marketContext.context_data_snapshots.length === 0 ? (
                                  <div className="muted">context data_snapshots: yok</div>
                                ) : (
                                  marketContext.context_data_snapshots.map((row) => (
                                    <div key={row.source_key} style={{ marginBottom: "0.25rem" }}>
                                      ctx <strong>{row.source_key}</strong>
                                      {row.error ? <span className="err"> {row.error}</span> : null}
                                      <br />
                                      {row.computed_at}
                                    </div>
                                  ))
                                )}
                              </>
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

              {drawerTab === "market_context" ? (
                <>
                  {token ? (
                    matchesSetting(
                      "bağlam",
                      "baglam",
                      "context",
                      "market",
                      "confluence",
                      "f7",
                      "summary",
                      "snapshot",
                      "özet",
                      "piyasa",
                      "latest",
                      "external",
                      "data",
                      "plan",
                      "weights",
                      "hl_meta",
                      "token_screener",
                      "source_key",
                      "external-fetch",
                      "kaynaklar",
                      "sources",
                      "tanım",
                      "sources",
                      "onchain",
                      "on-chain",
                      "türev",
                      "funding",
                    ) ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">Piyasa bağlamı (F7 / PLAN Phase E)</p>
                      <p className="muted" style={{ fontSize: "0.76rem", marginBottom: "0.45rem" }}>
                        Üst çubuk:{" "}
                        <span className="mono">
                          {barExchange.trim() || "—"}/{normalizeMarketSegment(barSegment)}/
                          {barSymbol.trim().toUpperCase() || "—"}/{barInterval.trim() || "—"}
                        </span>
                        . API: <code>market-context/latest</code>, <code>market-context/summary</code>,{" "}
                        <code>engine/confluence/latest</code>, <code>onchain-signals/breakdown</code>,{" "}
                        <code>data-snapshots</code>. Worker ortamı (confluence,
                        Nansen, harici çekim): repo kökü <code>.env.example</code>.{" "}
                        <code>source_key</code> adları: <code>docs/DATA_SOURCES_AND_SOURCE_KEYS.md</code>. Ayrıntı:{" "}
                        <code>docs/PLAN_CONFLUENCE_AND_MARKET_DATA.md</code>,{" "}
                        <code>docs/SPEC_EXECUTION_RANGE_SIGNALS_UI.md</code> (F7),{" "}
                        <code>docs/SPEC_ONCHAIN_SIGNALS.md</code>. Confluence:{" "}
                        <code>QTSS_CONFLUENCE_ENGINE</code> (0/kapalı ile kapatılır).
                      </p>
                      <button
                        type="button"
                        className="theme-toggle"
                        style={{ marginBottom: "0.5rem", fontSize: "0.78rem" }}
                        disabled={contextTabBusy}
                        onClick={() => void refreshMarketContextPanel()}
                      >
                        {contextTabBusy ? "Yenileniyor…" : "Şimdi yenile"}
                      </button>
                      {contextTabErr ? <p className="err">{contextTabErr}</p> : null}

                      <p className="tv-drawer__section-head" style={{ marginTop: "0.35rem" }}>
                        Motor hedefleri (filtreli özet)
                      </p>
                      <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.25rem" }}>
                        <code>GET …/market-context/summary</code> — Üst çubukta sembol varken aynı{" "}
                        <code>exchange</code> / <code>segment</code> / <code>symbol</code> ile süzülür; sembol boşken
                        tüm <strong>aktif</strong> hedefler (limit).
                      </p>
                      <div
                        className="mono muted"
                        style={{
                          maxHeight: "10rem",
                          overflow: "auto",
                          fontSize: "0.68rem",
                          marginBottom: "0.5rem",
                        }}
                      >
                        {contextTabSummaries.length === 0 ? (
                          <span className="muted">Özet satırı yok — motor hedefi ekleyin veya süzgeci gevşetin.</span>
                        ) : (
                          contextTabSummaries.map((r) => {
                            const cf = r.confluence;
                            let cLine = "—";
                            if (cf) {
                              const parts: string[] = [];
                              if (typeof cf.regime === "string" && cf.regime.length > 0) parts.push(cf.regime);
                              if (typeof cf.composite_score === "number") {
                                parts.push(`comp ${cf.composite_score.toFixed(3)}`);
                              }
                              if (typeof cf.confidence_0_100 === "number") {
                                parts.push(`conf ${Math.round(cf.confidence_0_100)}`);
                              }
                              if (parts.length > 0) {
                                cLine = parts.join(" · ");
                              } else if (cf.error) {
                                cLine = `err ${cf.error}`;
                              }
                            }
                            const previewLen = cf?.conflict_codes_preview?.length ?? 0;
                            const moreConf =
                              cf &&
                              typeof cf.conflicts_count === "number" &&
                              cf.conflicts_count > previewLen;
                            return (
                              <div key={r.engine_symbol_id} style={{ marginBottom: "0.28rem" }}>
                                <strong>
                                  {r.exchange}/{r.segment} {r.symbol} {r.interval}
                                </strong>
                                {!r.enabled ? <span className="muted"> (kapalı)</span> : null}
                                <br />
                                TA: {r.ta_durum ?? "—"} · piyasa_modu {r.ta_piyasa_modu ?? "—"}
                                <br />
                                Confluence: {cLine}
                                {cf && typeof cf.lot_scale_hint === "number"
                                  ? ` · lot_scale ${cf.lot_scale_hint.toFixed(2)}`
                                  : ""}
                                {cf && typeof cf.conflicts_count === "number" && cf.conflicts_count > 0 ? (
                                  <>
                                    {" "}
                                    · conflicts {cf.conflicts_count}
                                    {previewLen > 0
                                      ? ` (${cf.conflict_codes_preview!.join(", ")}${moreConf ? "…" : ""})`
                                      : ""}
                                  </>
                                ) : null}
                              </div>
                            );
                          })
                        )}
                      </div>

                      <p className="tv-drawer__section-head" style={{ marginTop: "0.5rem" }}>
                        Tek hedef özeti
                      </p>
                      <div
                        className="mono muted"
                        style={{ maxHeight: "11rem", overflow: "auto", fontSize: "0.72rem", marginBottom: "0.55rem" }}
                      >
                        {!barSymbol.trim() ? (
                          <span>Sembol seçin — üst çubuktan.</span>
                        ) : !contextTabSingle ? (
                          <span>
                            Bu hedef için <code>engine_symbols</code> yok veya 404 — Motor sekmesinden hedef ekleyin.
                          </span>
                        ) : (
                          <>
                            <div style={{ marginBottom: "0.35rem" }}>
                              <strong>
                                {contextTabSingle.exchange}/{contextTabSingle.segment} {contextTabSingle.symbol}{" "}
                                {contextTabSingle.interval}
                              </strong>
                            </div>
                            {(() => {
                              const dash = contextTabSingle.technical.signal_dashboard;
                              const d =
                                dash && typeof dash === "object" ? (dash as Record<string, unknown>) : null;
                              const p = d as SignalDashboardPayload | null;
                              const v2 = d ? parseSignalDashboardV2(d.signal_dashboard_v2) : null;
                              const durum = pickDashboardStr(
                                v2?.status,
                                typeof d?.durum === "string" ? d.durum : undefined,
                              );
                              const piyasa = pickDashboardStr(
                                v2?.market_mode,
                                typeof d?.piyasa_modu === "string" ? d.piyasa_modu : undefined,
                              );
                              const yerel = pickDashboardStr(v2?.local_trend, p?.yerel_trend);
                              const gbl = pickDashboardStr(v2?.global_trend, p?.global_trend);
                              return (
                                <div style={{ marginBottom: "0.3rem" }}>
                                  TA: <strong>durum</strong> {durum} · <strong>piyasa_modu</strong> {piyasa}
                                  <br />
                                  <span className="muted" style={{ fontSize: "0.68rem" }}>
                                    yerel_trend {yerel} · global_trend {gbl}
                                  </span>
                                </div>
                              );
                            })()}
                            {(() => {
                              const cf = contextTabSingle.confluence;
                              const p = cf && typeof cf === "object" ? (cf as Record<string, unknown>) : null;
                              if (!p) {
                                return <div className="muted">Confluence: —</div>;
                              }
                              const pillars = p.pillar_scores;
                              const pt =
                                pillars && typeof pillars === "object"
                                  ? (pillars as Record<string, unknown>)
                                  : null;
                              const comp =
                                typeof p.composite_score === "number" ? p.composite_score.toFixed(3) : "—";
                              const reg = typeof p.regime === "string" ? p.regime : "—";
                              const conf =
                                typeof p.confidence_0_100 === "number" ? String(p.confidence_0_100) : "—";
                              const extras = formatConfluenceExtras(p);
                              return (
                                <div style={{ marginBottom: "0.3rem" }}>
                                  <strong>Confluence</strong>: regime {reg} · composite {comp} · confidence {conf}
                                  {extras ? (
                                    <>
                                      <br />
                                      <span style={{ opacity: 0.92 }}>{extras.replace(/^ · /, "")}</span>
                                    </>
                                  ) : null}
                                  {pt ? (
                                    <>
                                      <br />
                                      pillars: technical{" "}
                                      {typeof pt.technical === "number" ? pt.technical.toFixed(2) : "—"} · onchain{" "}
                                      {typeof pt.onchain === "number" ? pt.onchain.toFixed(2) : "—"} · smart_money{" "}
                                      {typeof pt.smart_money === "number" ? pt.smart_money.toFixed(2) : "—"}
                                    </>
                                  ) : null}
                                </div>
                              );
                            })()}
                            {contextTabSingle.context_data_snapshots.length === 0 ? (
                              <div className="muted">İlgili data_snapshots: yok</div>
                            ) : (
                              contextTabSingle.context_data_snapshots.map((row) => (
                                <div key={row.source_key} style={{ marginBottom: "0.22rem" }}>
                                  <strong>{row.source_key}</strong>
                                  {row.error ? <span className="err"> {row.error}</span> : null} · {row.computed_at}
                                </div>
                              ))
                            )}
                          </>
                        )}
                      </div>

                      <p className="tv-drawer__section-head" style={{ marginTop: "0.45rem" }}>
                        On-chain skor (<code>onchain_signal_scores</code>)
                      </p>
                      <div
                        className="mono muted"
                        style={{ maxHeight: "10rem", overflow: "auto", fontSize: "0.7rem", marginBottom: "0.5rem" }}
                      >
                        {!barSymbol.trim() ? (
                          <span>Sembol seçin — üst çubuktan.</span>
                        ) : contextTabOnchainEndpointMissing ? (
                          <span className="muted">
                            <code>GET …/analysis/onchain-signals/breakdown</code> 404 — güncel{" "}
                            <code>qtss-api</code> ve migration <code>0030_onchain_signal_scores</code>.
                          </span>
                        ) : !contextTabOnchainBreakdown?.latest_score_row ? (
                          <span className="muted">
                            Henüz skor satırı yok — worker&apos;da{" "}
                            <code>QTSS_ONCHAIN_SIGNAL_ENGINE</code> ve tablo; bkz.{" "}
                            <code>docs/SPEC_ONCHAIN_SIGNALS.md</code>.
                          </span>
                        ) : (
                          (() => {
                            const row = contextTabOnchainBreakdown.latest_score_row;
                            const bd = contextTabOnchainBreakdown.onchain_breakdown;
                            const parts =
                              bd &&
                              typeof bd === "object" &&
                              bd !== null &&
                              "source_breakdown" in bd &&
                              Array.isArray((bd as { source_breakdown: unknown }).source_breakdown)
                                ? ((bd as { source_breakdown: unknown[] }).source_breakdown as unknown[])
                                : [];
                            return (
                              <>
                                <div style={{ marginBottom: "0.35rem" }}>
                                  <strong>{row.symbol}</strong> · <strong>{row.direction}</strong> · aggregate{" "}
                                  <strong>{row.aggregate_score.toFixed(3)}</strong> · confidence{" "}
                                  <strong>{(row.confidence * 100).toFixed(0)}%</strong>
                                  {row.conflict_detected ? <span className="err"> · conflict</span> : null}
                                  <br />
                                  <span style={{ opacity: 0.9 }}>
                                    {row.market_regime ? `regime ${row.market_regime} · ` : ""}
                                    {row.computed_at}
                                  </span>
                                </div>
                                {parts.length === 0 ? (
                                  <div className="muted">
                                    Bileşen dökümü yok (eski <code>meta_json</code>) — worker ile yeni tick.
                                  </div>
                                ) : (
                                  parts.map((p, i) => {
                                    if (!p || typeof p !== "object") return null;
                                    const o = p as Record<string, unknown>;
                                    const comp = typeof o.component === "string" ? o.component : "—";
                                    const sc = typeof o.score === "number" ? o.score.toFixed(2) : "—";
                                    const cf = typeof o.confidence === "number" ? o.confidence.toFixed(2) : "—";
                                    const wt = typeof o.weight === "number" ? o.weight.toFixed(2) : "—";
                                    const sk = typeof o.source_key === "string" ? o.source_key : "";
                                    return (
                                      <div key={`ocb-${i}-${comp}`} style={{ marginBottom: "0.22rem" }}>
                                        {comp}: score {sc} · conf {cf} · weight {wt}
                                        {sk ? (
                                          <>
                                            <br />
                                            <span style={{ opacity: 0.85 }}>{sk}</span>
                                          </>
                                        ) : null}
                                      </div>
                                    );
                                  })
                                )}
                              </>
                            );
                          })()
                        )}
                      </div>

                      <p className="tv-drawer__section-head">Tüm confluence satırları (motor)</p>
                      <div
                        className="mono muted"
                        style={{ maxHeight: "8rem", overflow: "auto", fontSize: "0.7rem", marginBottom: "0.55rem" }}
                      >
                        {contextTabConfluenceEndpointMissing ? (
                          <span className="muted">
                            <code>GET /api/v1/analysis/engine/confluence/latest</code> sunucuda yok (404) —{" "}
                            <code>qtss-api</code> güncel kodla derleyip yeniden başlatın. Özet ve{" "}
                            <code>data-snapshots</code> yine yüklendi. <code>VITE_API_BASE</code> sonuna{" "}
                            <code>/api/v1</code> eklemeyin.
                          </span>
                        ) : contextTabConfluence.length === 0 ? (
                          <span className="muted">Henüz confluence snapshot yok.</span>
                        ) : (
                          contextTabConfluence.map((s) => {
                            const p =
                              s.payload && typeof s.payload === "object"
                                ? (s.payload as Record<string, unknown>)
                                : null;
                            const comp =
                              typeof p?.composite_score === "number" ? p.composite_score.toFixed(3) : "—";
                            const reg = typeof p?.regime === "string" ? p.regime : "—";
                            const extras = p ? formatConfluenceExtras(p) : "";
                            return (
                              <div key={`ctx-cf-${s.engine_symbol_id}`} style={{ marginBottom: "0.28rem" }}>
                                {s.symbol} {s.interval} · {reg} · {comp}
                                {extras}
                                <br />
                                <span style={{ opacity: 0.85 }}>{s.computed_at}</span>
                              </div>
                            );
                          })
                        )}
                      </div>

                      <p className="tv-drawer__section-head">Harici HTTP kaynakları (`external-fetch/sources`)</p>
                      <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.3rem" }}>
                        Worker <code>QTSS_EXTERNAL_FETCH</code> bu tanımları okur; son yanıtlar{" "}
                        <code>data_snapshots</code> içinde. Yazma: ops rolü{" "}
                        <code>POST /api/v1/analysis/external-fetch/sources</code>.
                      </p>
                      <div
                        className="mono muted"
                        style={{ maxHeight: "7rem", overflow: "auto", fontSize: "0.66rem", marginBottom: "0.55rem" }}
                      >
                        {contextTabExternalSources.length === 0 ? (
                          <span>
                            Tanım yok veya tablo/migration eksik — <code>0021_external_data_fetch</code> sonrası seed / SQL.
                          </span>
                        ) : (
                          contextTabExternalSources.map((s) => {
                            const u = s.url.length > 72 ? `${s.url.slice(0, 70)}…` : s.url;
                            return (
                              <div key={s.key} style={{ marginBottom: "0.28rem" }}>
                                <strong>{s.key}</strong>
                                {s.enabled ? null : <span className="muted"> (kapalı)</span>}
                                <br />
                                {s.method} · tick {s.tick_secs}s
                                <br />
                                {u}
                              </div>
                            );
                          })
                        )}
                      </div>

                      <p className="tv-drawer__section-head">Birleşik data_snapshots (tam liste)</p>
                      <div
                        className="mono muted"
                        style={{ maxHeight: "9rem", overflow: "auto", fontSize: "0.68rem" }}
                      >
                        {contextTabDataSnaps.length === 0 ? (
                          <span>Henüz satır yok.</span>
                        ) : (
                          contextTabDataSnaps.map((d) => (
                            <div key={d.source_key} style={{ marginBottom: "0.25rem" }}>
                              <strong>{d.source_key}</strong>
                              {d.error ? <span className="err"> {d.error}</span> : null}
                              <br />
                              {d.computed_at}
                            </div>
                          ))
                        )}
                      </div>
                    </div>
                    ) : null
                  ) : (
                    <p className="muted">Piyasa bağlamı için giriş yap.</p>
                  )}
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
                          <code>NANSEN_TICK_SECS</code> — token screener çağrı aralığı (varsayılan 1800 sn); kredi için
                          yüksek tutun. Kredi bitince bekleme:{" "}
                          <code>NANSEN_INSUFFICIENT_CREDITS_SLEEP_SECS</code> (varsayılan 3600).
                        </li>
                        <li>
                          <code>QTSS_SETUP_SNAPSHOT_ONLY</code> — varsayılan <code>1</code>: setup <strong>ikinci</strong>{" "}
                          Nansen isteği yapmaz, yalnız <code>nansen_snapshots</code> okur (tek kredili çağrı{" "}
                          <code>nansen_engine</code>). Yedek canlı: <code>0</code>.
                        </li>
                        <li>
                          <code>NANSEN_TOKEN_SCREENER_REQUEST_JSON</code> — isteğe bağlı tam JSON; yoksa kod varsayılanı
                          (6h, <code>trader_type</code> sm, <code>per_page</code> 100, vb.).
                        </li>
                        <li>
                          <code>NANSEN_API_BASE</code> — varsayılan <code>https://api.nansen.ai</code>.
                        </li>
                        <li>
                          API: <code>GET …/analysis/nansen/snapshot</code> ve{" "}
                          <code>GET …/analysis/nansen/setups/latest</code> (JWT) — aşağıda özet + setup tablosu.
                        </li>
                        <li>
                          <code>QTSS_SETUP_SCAN_SECS</code> — setup tarama aralığı (varsayılan 900 sn; snapshot-only iken
                          ek Nansen kredisi tüketmez). <code>QTSS_SETUP_MAX_SNAPSHOT_AGE_SECS</code> yalnız{" "}
                          <code>QTSS_SETUP_SNAPSHOT_ONLY=0</code> iken canlı yedek kararını etkiler.
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

              {drawerTab === "queues" ? (
                <>
                  {matchesSetting(
                    "kuyruk",
                    "queue",
                    "notify",
                    "outbox",
                    "bildirim",
                    "ai",
                    "onay",
                    "approval",
                    "ops",
                    "worker",
                  ) ? (
                    <OperationsQueuesPanel
                      accessToken={token}
                      canOps={rbacIsOps}
                      canAdmin={rbacIsAdmin}
                    />
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
