import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { binanceKlinesUsesQtssApi, fetchBinanceKlinesAsChartRows } from "./api/binanceKlines";
import {
  backfillMarketBarsFromRest,
  fetchChartPatternsConfig,
  fetchConfigList,
  fetchElliottWaveConfig,
  fetchHealth,
  fetchMarketBarsRecent,
  fetchWebOAuthBootstrap,
  oauthTokenPassword,
  oauthTokenRefresh,
  scanChannelSix,
  upsertAppConfig,
  type ChannelSixResponse,
  fetchAuthMe,
  configureApiAuth,
  type DataSnapshotApiRow,
  type ExternalDataSourceApiRow,
  fetchEngineSnapshots,
  fetchEngineSymbolIngestion,
  fetchDataSnapshots,
  fetchExternalFetchSources,
  fetchMarketContextLatest,
  fetchMarketContextSummary,
  fetchOnchainSignalsBreakdown,
  fetchConfluenceSnapshotsLatest,
  fetchEngineRangeSignals,
  fetchRangeEngineConfig,
  patchRangeEngineConfig,
  fetchEngineSymbols,
  fetchNansenSnapshot,
  fetchNansenSetupsLatest,
  fetchPaperBalance,
  fetchPaperFills,
  fetchMyExchangeFills,
  fetchAdminSystemConfig,
  upsertAdminSystemConfig,
  fetchBinanceCommissionDefaults,
  fetchBinanceCommissionAccount,
  fetchCatalogExchanges,
  fetchInstrumentSuggestions,
  postEngineSymbol,
  patchEngineSymbol,
  type EngineSnapshotJoinedApiRow,
  type EngineSymbolApiRow,
  type EngineSymbolIngestionApiRow,
  type MarketContextLatestApiResponse,
  type MarketContextSummaryItemApi,
  type NansenSetupsLatestApiResponse,
  type NansenSnapshotApiRow,
  type PaperBalanceRow,
  type PaperFillRow,
  type ExchangeFillRowApi,
  type SystemConfigRowApi,
  type BinanceCommissionDefaultsApi,
  type BinanceCommissionAccountApi,
  type CatalogExchangeRowApi,
  type RangeSignalEventApiRow,
  type OnchainSignalsBreakdownApi,
  type RangeEngineConfigApi,
} from "./api/client";
import { channelDrawingToOverlay } from "./lib/channelOverlayFromDrawing";
import { buildChannelScanPivotMarkers } from "./lib/channelScanMarkers";
import {
  buildMultiPatternOverlayFromScan,
  type PatternLayerOverlay,
  type MultiPatternChartOverlay,
} from "./lib/patternDrawingBatchOverlay";
import { ChannelScanMatchesTable } from "./components/ChannelScanMatchesTable";
import { AiDecisionsPanel } from "./components/AiDecisionsPanel";
import { LanguageSwitcher } from "./components/LanguageSwitcher";
import { OperationsQueuesPanel } from "./components/OperationsQueuesPanel";
import i18n from "./i18n";
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
import { SignalDashboardDrawerPanel } from "./components/SignalDashboardDrawerPanel";
import { TradeDashboardPanel } from "./components/TradeDashboardPanel";
import {
  DEFAULT_ELLIOTT_WAVE_CONFIG,
  ELLIOTT_WAVE_CONFIG_KEY,
  mtfWaveColorsFromConfig,
  mtfZigzagColorsFromConfig,
  normalizeElliottWaveConfig,
  patternMenuForTf,
  scaleElliottHexColor,
  type ElliottWaveConfig,
} from "./lib/elliottWaveAppConfig";
import { ELLIOTT_PATTERN_MENU_ROWS } from "./lib/elliottPatternMenuCatalog";
import { buildElliottLegendRows } from "./lib/elliottWaveLegend";
import { buildSwingPivots } from "./lib/elliottImpulseDetect";
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
import { rangeSetupsFromEvents } from "./lib/rangeSetupsFromEvents";
import { rangeSignalMarkersFromEvents } from "./lib/rangeSignalMarkers";
import { patternLayerFromDbTradingRange, sweepMarkersFromDbTradingRange, type TradingRangeDbPayload } from "./lib/tradingRangeDbOverlay";
import {
  formatDashboardNumber,
  parseSignalDashboardV2,
  pickDashboardBool,
  pickDashboardNum,
  pickDashboardStr,
  trendAxisDisplayAsLongShort,
  type SignalDashboardPayload,
} from "./lib/signalDashboardPayload";
import { canAdmin, canOps, type AuthSession } from "./lib/rbac";
import type { SeriesMarker, UTCTimestamp } from "lightweight-charts";
import { BacktestRunCard } from "./components/BacktestRunCard";
import { OrdersAndFillsCard } from "./components/OrdersAndFillsCard";
import { TradingRangeSetupEngineSymbolsPanel } from "./components/TradingRangeSetupEngineSymbolsPanel";
import { TradingRangeSetupTable } from "./components/TradingRangeSetupTable";
import { TradingRangeTradeSummary } from "./components/TradingRangeTradeSummary";
import { TradingRangeFuturesQuickTrade } from "./components/TradingRangeFuturesQuickTrade";
import { TradingRangeDataEntryPanel } from "./components/TradingRangeDataEntryPanel";
import { NotificationDrawerPanel } from "./components/NotificationDrawerPanel";
import { ServerRegistryPanel } from "./components/ServerRegistryPanel";
import { HelpCrossLink } from "./help/HelpCrossLink";
import { HelpPanel } from "./help/HelpPanel";
import {
  TRADING_RANGE_DRAWER_REFRESH_MS,
  SIGNAL_DASHBOARD_DRAWER_REFRESH_MS,
} from "./app/drawerRefreshConstants";
import type { Theme, SettingsTab, TradingRangeDrawerSubtab, ElliottLineStyle } from "./app/appTypes";
import { readChartDefaults, readLivePollMs } from "./app/chartEnv";
import { normalizeMarketSegment, chartToolbarSegmentSelectValue } from "./app/marketSegment";
import {
  keepElliottZigzagLayer,
  isV2RawZigzagKind,
  patchPatternMenuTf,
  elliottColorInputValue,
} from "./app/elliottAppHelpers";
import { formatConfluenceExtras } from "./app/confluenceFormat";
import { channelSixRejectMessage } from "./app/channelRejectMessage";
import {
  ACCESS_TOKEN_STORAGE_KEY,
  ACCESS_TOKEN_EXP_MS_STORAGE_KEY,
  REFRESH_TOKEN_STORAGE_KEY,
  readStoredAccessToken,
  readStoredRefreshToken,
  readStoredAccessExpMs,
} from "./app/oauthStorage";

export default function App() {
  const { t } = useTranslation();
  const defaults = readChartDefaults();
  const [theme, setTheme] = useState<Theme>(() => {
    if (typeof window === "undefined") return "dark";
    const s = localStorage.getItem("qtss-theme") as Theme | null;
    return s === "dark" || s === "light" ? s : "dark";
  });

  const [drawerOpen, setDrawerOpen] = useState(false);
  const [drawerTab, setDrawerTab] = useState<SettingsTab>("general");
  const [tradingRangeSubtab, setTradingRangeSubtab] = useState<TradingRangeDrawerSubtab>("main");
  const [drawerSearch, setDrawerSearch] = useState("");
  const [helpFocusId, setHelpFocusId] = useState<string | null>(null);
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
  /** OAuth client id/secret from `GET /api/v1/bootstrap/web-oauth-client` (`system_config` seed). */
  const [webOauthBootstrap, setWebOauthBootstrap] = useState<{
    clientId: string;
    clientSecret: string;
    suggestedLoginEmail: string;
  } | null>(null);
  const [loginEmail, setLoginEmail] = useState("");
  const [loginPassword, setLoginPassword] = useState("");

  useEffect(() => {
    let cancelled = false;
    void fetchWebOAuthBootstrap()
      .then((b) => {
        if (cancelled) return;
        setWebOauthBootstrap({
          clientId: b.clientId,
          clientSecret: b.clientSecret,
          suggestedLoginEmail: b.suggestedLoginEmail,
        });
        setLoginEmail((prev) => (prev.trim() ? prev : b.suggestedLoginEmail || ""));
      })
      .catch(() => {
        /* fall back to VITE_* in configureApiAuth / tryDevLogin */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (typeof window === "undefined") return;
    try {
      if (token) {
        localStorage.setItem(ACCESS_TOKEN_STORAGE_KEY, token);
      } else {
        localStorage.removeItem(ACCESS_TOKEN_STORAGE_KEY);
        localStorage.removeItem(ACCESS_TOKEN_EXP_MS_STORAGE_KEY);
        localStorage.removeItem(REFRESH_TOKEN_STORAGE_KEY);
      }
    } catch {
      /* private mode, quota */
    }
  }, [token]);

  useEffect(() => {
    configureApiAuth({
      getRefreshToken: readStoredRefreshToken,
      setTokens: (accessToken, refreshToken) => {
        setToken(accessToken);
        try {
          if (typeof window === "undefined") return;
          const rt = refreshToken?.trim() ?? "";
          if (rt) localStorage.setItem(REFRESH_TOKEN_STORAGE_KEY, rt);
          else localStorage.removeItem(REFRESH_TOKEN_STORAGE_KEY);
        } catch {
          /* private mode, quota */
        }
      },
      getOAuthClientCredentials: () => ({
        clientId: webOauthBootstrap?.clientId ?? import.meta.env.VITE_OAUTH_CLIENT_ID ?? "",
        clientSecret:
          webOauthBootstrap?.clientSecret ?? import.meta.env.VITE_OAUTH_CLIENT_SECRET ?? "",
      }),
    });
    return () => {
      configureApiAuth(null);
    };
  }, [webOauthBootstrap]);

  // Proactive refresh: refresh access_token shortly before expiry.
  useEffect(() => {
    if (typeof window === "undefined") return;
    if (!token) return;
    let cancelled = false;
    let to: number | null = null;
    async function arm() {
      const expMs = readStoredAccessExpMs();
      const rt = readStoredRefreshToken();
      const clientId =
        webOauthBootstrap?.clientId ?? import.meta.env.VITE_OAUTH_CLIENT_ID ?? "";
      const clientSecret =
        webOauthBootstrap?.clientSecret ?? import.meta.env.VITE_OAUTH_CLIENT_SECRET ?? "";
      if (!expMs || !rt || !clientId || !clientSecret) return;
      const now = Date.now();
      const refreshAt = expMs - 30_000; // 30s early
      const delay = Math.max(1000, refreshAt - now);
      to = window.setTimeout(async () => {
        try {
          const tr = await oauthTokenRefresh({ refreshToken: rt, clientId, clientSecret });
          if (cancelled) return;
          setToken(tr.access_token);
          try {
            const nextRt = tr.refresh_token?.trim() ? tr.refresh_token.trim() : rt.trim();
            localStorage.setItem(REFRESH_TOKEN_STORAGE_KEY, nextRt);
            localStorage.setItem(
              ACCESS_TOKEN_EXP_MS_STORAGE_KEY,
              String(Date.now() + Math.max(0, (tr.expires_in ?? 0) * 1000)),
            );
          } catch {
            /* ignore */
          }
        } catch {
          // If refresh fails, force logout so UI doesn't spam 401 forever.
          if (!cancelled) setToken(null);
        }
      }, delay);
    }
    void arm();
    return () => {
      cancelled = true;
      if (to != null) window.clearTimeout(to);
    };
  }, [token, webOauthBootstrap]);
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
  const [catalogExchanges, setCatalogExchanges] = useState<CatalogExchangeRowApi[]>([]);
  const [symbolDatalist, setSymbolDatalist] = useState<string[]>([]);
  const [chartOhlcMode, setChartOhlcMode] = useState<ChartOhlcMode>(() => readChartOhlcMode());
  const [acpConfig, setAcpConfig] = useState<AcpChartPatternsConfig>(() => ({ ...DEFAULT_ACP_CONFIG }));
  const [acpConfigLoadErr, setAcpConfigLoadErr] = useState("");
  const [acpSaveErr, setAcpSaveErr] = useState("");
  const [acpSaveBusy, setAcpSaveBusy] = useState(false);
  const [channelScanLoading, setChannelScanLoading] = useState(false);
  const [channelScanJson, setChannelScanJson] = useState<string>("");
  const [channelScanError, setChannelScanError] = useState<string>("");
  const [lastChannelScan, setLastChannelScan] = useState<ChannelSixResponse | null>(null);
  /** OHLC window sent to the last successful `scanChannelSix` call — keeps `bar_index` aligned after live poll. */
  const [lastChannelScanBars, setLastChannelScanBars] = useState<ChartOhlcRow[] | null>(null);
  const [channelScanSummary, setChannelScanSummary] = useState<string>("");
  const [channelScanHoverTitle, setChannelScanHoverTitle] = useState<string>("");
  /** Sembol/interval tam yükleme sonrası artar — TvChartPane yalnızca bu değişince `fitContent`. */
  const [chartFitKey, setChartFitKey] = useState(0);

  /** Aynı anda birden fazla yükleme: yalnızca son isteğin cevabı `setBars` uygular (BTC→ETH yarışı). */
  const chartLoadSeqRef = useRef(0);
  const livePollEpochRef = useRef(0);
  const symbolSuggestTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  type ProjectionGuardState = {
    anchorTimeSec: number;
    anchorBarCount: number;
    invalidReason?: string;
    impulseKey?: string;
  };
  const projectionGuardRef = useRef<Record<"4h" | "1h" | "15m", ProjectionGuardState>>({
    "4h": { anchorTimeSec: 0, anchorBarCount: 0 },
    "1h": { anchorTimeSec: 0, anchorBarCount: 0 },
    "15m": { anchorTimeSec: 0, anchorBarCount: 0 },
  });

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
              ? t("app.channelScan.errorInsufficientRepaint")
              : t("app.channelScan.errorInsufficientNoRepaint"),
          );
          setLastChannelScan(null);
          setLastChannelScanBars(null);
          setChannelScanJson("");
          setChannelScanSummary("");
          setChannelScanHoverTitle("");
          return;
        }
        const payload = chartOhlcRowsToScanBars(scanWindow);
        const base = acpConfigToChannelSixOptions(acpConfig, theme);
        const res = await scanChannelSix(token, { bars: payload, ...(base as Record<string, unknown>) });
        setLastChannelScan(res);
        setLastChannelScanBars(scanWindow.slice());
        setChannelScanJson(JSON.stringify(res, null, 2));
        if (res.matched && res.outcome) {
          const id = res.outcome.scan.pattern_type_id;
          const name = res.pattern_name ?? `id ${id}`;
          const sk = res.outcome.pivot_tail_skip ?? 0;
          const skipNote = sk > 0 ? t("app.channelScan.segmentPivotSkip", { count: sk }) : "";
          const lvl = res.outcome.zigzag_level ?? 0;
          const lvlNote = lvl > 0 ? t("app.channelScan.segmentLevel", { level: lvl }) : "";
          const zzNote = res.used_zigzag
            ? t("app.channelScan.segmentZg", {
                len: res.used_zigzag.length,
                depth: res.used_zigzag.depth,
              })
            : "";
          const nMatch = res.pattern_matches?.length ?? 1;
          const multiNames =
            nMatch > 1
              ? (res.pattern_matches ?? [])
                  .slice(0, 5)
                  .map((m) => m.pattern_name ?? `id ${m.outcome.scan.pattern_type_id}`)
                  .join(" · ")
              : "";
          const multiTail = nMatch > 5 ? "…" : "";
          const multi =
            nMatch > 1
              ? t("app.channelScan.segmentMultiFormations", {
                  count: nMatch,
                  names: `${multiNames}${multiTail}`,
                })
              : "";
          const hoverNames =
            res.pattern_matches?.map((m) => m.pattern_name ?? `id ${m.outcome.scan.pattern_type_id}`).join(" · ") ??
            name;
          const closedBarSuffix = acpConfig.scanning.repaint ? "" : t("app.channelScan.closedBarSuffix");
          const barRatioNote = !acpConfig.scanning.verify_bar_ratio
            ? "BR:Off"
            : Math.abs(acpConfig.scanning.bar_ratio_limit - 0.382) < 1e-6
              ? "BR:Strict"
              : Math.abs(acpConfig.scanning.bar_ratio_limit - 0.25) < 1e-6
                ? "BR:Relaxed"
                : `BR:${acpConfig.scanning.bar_ratio_limit.toFixed(3)}`;
          setChannelScanHoverTitle(t("app.channelScan.hoverFormations", { names: hoverNames }));
          setChannelScanSummary(
            t("app.channelScan.summaryMatch", {
              name,
              pickUpper: res.outcome.scan.pick_upper,
              pickLower: res.outcome.scan.pick_lower,
              pivots: res.outcome.zigzag_pivot_count,
              skipNote,
              lvlNote,
              zzNote,
              multi,
              closedNote: closedBarSuffix,
            }) + ` · ${barRatioNote}`,
          );
        } else {
          setChannelScanHoverTitle("");
          const closedBarSuffix = acpConfig.scanning.repaint ? "" : t("app.channelScan.closedBarSuffix");
          const reason = channelSixRejectMessage(t, res.reject);
          const barRatioHint =
            (res.reject?.code === "bar_ratio_upper" || res.reject?.code === "bar_ratio_lower") && acpConfig.scanning.verify_bar_ratio
              ? ` ${t("app.channelReject.barRatioHint", { limit: String(acpConfig.scanning.bar_ratio_limit) })}`
              : "";
          const insufficientHint =
            res.reject?.code === "insufficient_pivots"
              ? ` ${t("app.channelReject.insufficientPivotsHint")}`
              : "";
          setChannelScanSummary(
            t("app.channelScan.noMatchLine", {
              reason,
              bars: res.bar_count,
              pivots: res.zigzag_pivot_count,
              closedNote: closedBarSuffix,
            }) +
              ` · ${!acpConfig.scanning.verify_bar_ratio ? "BR:Off" : `BR:${acpConfig.scanning.bar_ratio_limit.toFixed(3)}`}` +
              insufficientHint +
              barRatioHint,
          );
        }
      } catch (e) {
        setChannelScanError(String(e));
        setLastChannelScan(null);
        setLastChannelScanBars(null);
        setChannelScanSummary("");
        setChannelScanHoverTitle("");
      } finally {
        setChannelScanLoading(false);
      }
    },
    [t, i18n.language, token, acpConfig, theme],
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
  const [engineIngestionRows, setEngineIngestionRows] = useState<EngineSymbolIngestionApiRow[]>([]);
  const [enginePanelErr, setEnginePanelErr] = useState("");
  const [engineFormSymbol, setEngineFormSymbol] = useState("");
  const [engineFormInterval, setEngineFormInterval] = useState("4h");
  const [engineFormBusy, setEngineFormBusy] = useState(false);
  const [engineListRefreshing, setEngineListRefreshing] = useState(false);
  const [rangeEngineConfig, setRangeEngineConfig] = useState<RangeEngineConfigApi | null>(null);
  const [rangeEnginePatchBusy, setRangeEnginePatchBusy] = useState(false);
  const [trParamsDraft, setTrParamsDraft] = useState({
    lookback: "",
    atr_period: "",
    atr_sma_period: "",
    require_range_regime: false,
  });
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
  const [exchangeFills, setExchangeFills] = useState<ExchangeFillRowApi[]>([]);
  const [workerFlags, setWorkerFlags] = useState<{
    nansenEnabled: boolean;
    externalFetchEnabled: boolean;
  }>({ nansenEnabled: true, externalFetchEnabled: true });
  const [commissionDefaults, setCommissionDefaults] = useState<BinanceCommissionDefaultsApi | null>(null);
  const [commissionAccount, setCommissionAccount] = useState<BinanceCommissionAccountApi | null>(null);
  const [commissionAccountErr, setCommissionAccountErr] = useState("");
  const [commissionAccountBusy, setCommissionAccountBusy] = useState(false);
  const [showDbTradingRangeLayer, setShowDbTradingRangeLayer] = useState(true);
  const [showDbSweepMarkers, setShowDbSweepMarkers] = useState(true);
  const [showDbRangeSignalMarkers, setShowDbRangeSignalMarkers] = useState(true);
  const [showDbOpenPositionLine, setShowDbOpenPositionLine] = useState(true);
  const [signalDashboardAutoRefresh, setSignalDashboardAutoRefresh] = useState(false);
  const [tradingRangeAutoRefresh, setTradingRangeAutoRefresh] = useState(false);
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
    if (!rangeEngineConfig?.trading_range_params) return;
    const p = rangeEngineConfig.trading_range_params;
    setTrParamsDraft({
      lookback: p.lookback != null && Number.isFinite(p.lookback) ? String(p.lookback) : "",
      atr_period: p.atr_period != null && Number.isFinite(p.atr_period) ? String(p.atr_period) : "",
      atr_sma_period:
        p.atr_sma_period != null && Number.isFinite(p.atr_sma_period) ? String(p.atr_sma_period) : "",
      require_range_regime: p.require_range_regime === true,
    });
  }, [rangeEngineConfig]);

  useEffect(() => {
    if (!token) {
      setAuthSession(null);
      setAuthMeErr("");
      setAuthMeLoading(false);
      setRangeEngineConfig(null);
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

  useEffect(() => {
    const pl = authSession?.preferredLocale;
    if (pl === "en" || pl === "tr") {
      void i18n.changeLanguage(pl);
    }
  }, [authSession?.preferredLocale]);

  useEffect(() => {
    if (!token?.trim()) {
      setCatalogExchanges([]);
      return;
    }
    let cancelled = false;
    void fetchCatalogExchanges(token)
      .then((rows) => {
        if (!cancelled) setCatalogExchanges(rows.filter((e) => e.is_active));
      })
      .catch(() => {
        if (!cancelled) setCatalogExchanges([]);
      });
    return () => {
      cancelled = true;
    };
  }, [token]);

  useEffect(() => {
    if (!token?.trim()) {
      setSymbolDatalist([]);
      return;
    }
    const raw = barSymbol.trim();
    const alnum = raw.replace(/[^A-Za-z0-9]/g, "");
    if (alnum.length < 1) {
      setSymbolDatalist([]);
      return;
    }
    if (symbolSuggestTimerRef.current != null) window.clearTimeout(symbolSuggestTimerRef.current);
    symbolSuggestTimerRef.current = window.setTimeout(() => {
      void fetchInstrumentSuggestions(token, {
        exchangeCode: barExchange.trim() || "binance",
        segment: barSegment.trim() || "spot",
        query: alnum,
        limit: 40,
      })
        .then((rows) => setSymbolDatalist(rows.map((r) => r.native_symbol)))
        .catch(() => setSymbolDatalist([]));
    }, 250);
    return () => {
      if (symbolSuggestTimerRef.current != null) window.clearTimeout(symbolSuggestTimerRef.current);
    };
  }, [token, barSymbol, barExchange, barSegment]);

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

  const elliottPanelSwingPivotCount = useMemo(() => {
    if (!elliottConfig.enabled || !bars?.length) return 0;
    return buildSwingPivots(chartOhlcRowsSortedChrono(bars), chartElliottZigzagDepth).length;
  }, [elliottConfig.enabled, bars, chartElliottZigzagDepth]);

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
    const lastRow = rows[rows.length - 1]!;
    const wc = mtfWaveColorsFromConfig(elliottConfig);
    const opt = {
      includeAltScenario: elliottConfig.show_projection_alt_scenario,
      multiCorrectiveScenarios: elliottConfig.projection_multi_corrective_scenarios,
    };
    const out: PatternLayerOverlay[] = [];
    const chartProjectionTf: "4h" | "1h" | "15m" =
      barInterval.trim() === "4h" ? "4h" : barInterval.trim() === "1h" ? "1h" : "15m";
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
      if (!on || tf !== chartProjectionTf) continue;
      // Guard: invalidation + timeout tracking per TF (stateful across renders via ref).
      const s = elliottV2Output.states[tf];
      const imp = s?.impulse ?? null;
      const g = projectionGuardRef.current[tf];
      // Match wave overlay: adapter draws `s.impulse` without requiring `decision !== "invalid"`.
      if (!imp) {
        g.invalidReason = "invalid";
        continue;
      }
      const key = imp
        ? `${imp.direction}|${imp.variant ?? "standard"}|${imp.pivots.map((p) => `${p.index}:${p.price.toFixed(4)}`).join(",")}`
        : "";
      if (key && g.impulseKey !== key) {
        g.impulseKey = key;
        g.anchorTimeSec = lastRow.t;
        g.anchorBarCount = rows.length;
        g.invalidReason = undefined;
      }
      // Timeout: after 100 bars since projection anchor, stop drawing.
      const barsSince = g.anchorBarCount > 0 ? Math.max(0, rows.length - g.anchorBarCount) : 0;
      if (barsSince > 100) {
        g.invalidReason = "timeout";
      }
      // Invalidation: classic wave4-overlap style guard for projections (simple heuristic).
      if (!g.invalidReason && imp && imp.pivots.length >= 2) {
        const p1 = imp.pivots[1];
        if (imp.direction === "bull") {
          if (lastRow.l <= p1.price) g.invalidReason = "wave4_overlap_like";
        } else {
          if (lastRow.h >= p1.price) g.invalidReason = "wave4_overlap_like";
        }
      }
      if (g.invalidReason) continue;
      const built = buildElliottProjectionOverlayV2(
        elliottV2Output,
        rows,
        opt,
        patternMenuForTf(elliottConfig, tf),
        wc[tf],
        tf,
      );
      if (built?.layers?.length) {
        const baseColor = wc[tf];
        out.push(
          ...built.layers.map((layer) => ({
            ...layer,
            zigzagLineColor:
              layer.zigzagKind === "elliott_projection_alt"
                ? scaleElliottHexColor(baseColor, 0.58)
                : (layer.zigzagLineColor ?? baseColor),
            zigzagLineStyle:
              layer.zigzagLineStyle ??
              (tf === "4h"
                ? elliottConfig.mtf_line_style_4h
                : tf === "1h"
                  ? elliottConfig.mtf_line_style_1h
                  : elliottConfig.mtf_line_style_15m),
            zigzagLineWidth:
              typeof layer.zigzagLineWidth === "number" && Number.isFinite(layer.zigzagLineWidth)
                ? layer.zigzagLineWidth
                : tf === "4h"
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
    elliottConfig.projection_multi_corrective_scenarios,
    elliottConfig.show_projection_alt_scenario,
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
    barInterval,
  ]);

  const elliottProjectionStatus = useMemo((): string => {
    if (!elliottConfig.enabled || !bars?.length) return "";
    const chartProjectionTf: "4h" | "1h" | "15m" =
      barInterval.trim() === "4h" ? "4h" : barInterval.trim() === "1h" ? "1h" : "15m";
    const parts: string[] = [];
    const specs: Array<{ tf: "4h" | "1h" | "15m"; on: boolean }> = [
      { tf: "4h", on: !!elliottConfig.show_projection_4h },
      { tf: "1h", on: !!elliottConfig.show_projection_1h },
      { tf: "15m", on: !!elliottConfig.show_projection_15m },
    ];
    for (const { tf, on } of specs) {
      if (!on || tf !== chartProjectionTf) continue;
      const r = projectionGuardRef.current[tf]?.invalidReason;
      if (!r) continue;
      const label = r === "timeout" ? "timeout" : r === "invalid" ? "invalid" : "invalid";
      parts.push(`${tf}:${label}`);
    }
    return parts.length ? `Elliott projection: ${parts.join(" · ")}` : "";
  }, [
    bars,
    barInterval,
    elliottConfig.enabled,
    elliottConfig.show_projection_15m,
    elliottConfig.show_projection_1h,
    elliottConfig.show_projection_4h,
  ]);

  const acpBarRatioModeLabel = useMemo((): "Strict" | "Relaxed" | "Off" | "Custom" => {
    if (!acpConfig.scanning.verify_bar_ratio) return "Off";
    const v = acpConfig.scanning.bar_ratio_limit;
    if (Math.abs(v - 0.382) < 1e-6) return "Strict";
    if (Math.abs(v - 0.25) < 1e-6) return "Relaxed";
    return "Custom";
  }, [acpConfig.scanning.bar_ratio_limit, acpConfig.scanning.verify_bar_ratio]);

  const cycleAcpBarRatioMode = useCallback(() => {
    setAcpConfig((prev) => {
      const cur = prev.scanning.verify_bar_ratio
        ? Math.abs(prev.scanning.bar_ratio_limit - 0.382) < 1e-6
          ? "Strict"
          : Math.abs(prev.scanning.bar_ratio_limit - 0.25) < 1e-6
            ? "Relaxed"
            : "Custom"
        : "Off";
      const next = cur === "Strict" ? "Relaxed" : cur === "Relaxed" ? "Off" : "Strict";
      const scanning =
        next === "Off"
          ? { ...prev.scanning, verify_bar_ratio: false }
          : next === "Relaxed"
            ? { ...prev.scanning, verify_bar_ratio: true, bar_ratio_limit: 0.25 }
            : { ...prev.scanning, verify_bar_ratio: true, bar_ratio_limit: 0.382 };
      return { ...prev, scanning };
    });
  }, [setAcpConfig]);

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
    return buildElliottLegendRows(
      elliottV2Output,
      anyElliottProjection,
      anyElliottProjection,
    );
  }, [anyElliottProjection, elliottV2Output]);

  const channelScanOverlayBars = useMemo((): ChartOhlcRow[] | null => {
    if (!lastChannelScan?.matched) return null;
    // Avoid bar_index drift after live poll: only draw overlays on the exact OHLC window used for the scan.
    if (!lastChannelScanBars?.length) return null;
    const scanLen = Math.min(lastChannelScan.bar_count, lastChannelScanBars.length);
    return scanLen > 0 ? lastChannelScanBars.slice(-scanLen) : lastChannelScanBars;
  }, [lastChannelScan, lastChannelScanBars]);

  const multiOverlay = useMemo(() => {
    if (!lastChannelScan?.matched) return null;
    const scanBars = channelScanOverlayBars;
    if (!scanBars?.length) return null;
    const chartChrono = bars?.length ? chartOhlcRowsSortedChrono(bars) : [];
    const fromMatches = buildMultiPatternOverlayFromScan(
      lastChannelScan,
      scanBars,
      acpConfig.display,
      chartChrono.length ? chartChrono : undefined,
    );
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
  }, [lastChannelScan, channelScanOverlayBars, acpConfig.display, bars]);

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

  const signalDashboardSnapshots = useMemo(() => {
    return engineSnapshots
      .filter((s) => s.engine_kind === "signal_dashboard")
      .slice()
      .sort((a, b) => {
        const ex = a.exchange.localeCompare(b.exchange);
        if (ex !== 0) return ex;
        const seg = a.segment.localeCompare(b.segment);
        if (seg !== 0) return seg;
        const sym = a.symbol.localeCompare(b.symbol);
        if (sym !== 0) return sym;
        return a.interval.localeCompare(b.interval);
      });
  }, [engineSnapshots]);

  /** Süpürme okları: öncelik `trading_range` yükü; yoksa `signal_dashboard` (aynı turda yazılan süpürme alanları). */
  const sweepMarkersPayload = useMemo(
    () => dbTradingRangeSnapshot?.payload ?? dbSignalDashboardSnapshot?.payload ?? null,
    [dbTradingRangeSnapshot, dbSignalDashboardSnapshot],
  );

  const tradingRangeScorePayload = useMemo((): TradingRangeDbPayload | null => {
    const p = (dbTradingRangeSnapshot?.payload ?? dbSignalDashboardSnapshot?.payload ?? null) as unknown;
    if (!p || typeof p !== "object") return null;
    return p as TradingRangeDbPayload;
  }, [dbTradingRangeSnapshot, dbSignalDashboardSnapshot]);

  const dbSweepMarkers = useMemo((): SeriesMarker<UTCTimestamp>[] => {
    if (!showDbSweepMarkers || !bars?.length || !sweepMarkersPayload) return [];
    return sweepMarkersFromDbTradingRange(bars, sweepMarkersPayload);
  }, [showDbSweepMarkers, bars, sweepMarkersPayload]);

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

  const rangeSignalMarkerLabel = useCallback(
    (kind: string) => {
      switch (kind) {
        case "long_entry":
          return t("app.rangeSignalMarkers.longEntry");
        case "long_exit":
          return t("app.rangeSignalMarkers.longExit");
        case "short_entry":
          return t("app.rangeSignalMarkers.shortEntry");
        case "short_exit":
          return t("app.rangeSignalMarkers.shortExit");
        default:
          return kind;
      }
    },
    [t],
  );

  const dbRangeSignalMarkers = useMemo((): SeriesMarker<UTCTimestamp>[] => {
    if (!showDbRangeSignalMarkers || !bars?.length) return [];
    return rangeSignalMarkersFromEvents(bars, engineChartRangeSignalEvents, rangeSignalMarkerLabel);
  }, [showDbRangeSignalMarkers, bars, engineChartRangeSignalEvents, rangeSignalMarkerLabel]);

  const chartRangeSetups = useMemo(
    () => rangeSetupsFromEvents(engineChartRangeSignalEvents),
    [engineChartRangeSignalEvents],
  );

  /** Taker kesiri (giriş+çıkış) — hesap yüklemesi varsa onu, yoksa Binance varsayılan bps. */
  const tradingRangeTakerFraction = useMemo((): number | null => {
    if (commissionAccount) {
      const t = Number(commissionAccount.taker_rate);
      if (Number.isFinite(t) && t >= 0) return t;
    }
    if (commissionDefaults) {
      const t = commissionDefaults.defaults_bps.taker_bps / 10_000;
      if (Number.isFinite(t) && t >= 0) return t;
    }
    return null;
  }, [commissionAccount, commissionDefaults]);

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
    const cap = 40;
    const elayersRaw: PatternLayerOverlay[] = elliottChartBundle?.layers ?? [];
    const elayers = elayersRaw.filter((l) => keepElliottZigzagLayer(l.zigzagKind, elliottConfig));
    const proj = elliottProjectionLayers;
    const eAll = proj.length ? [...elayers, ...proj] : [...elayers];
    // ACP first: old logic reserved only (cap - eAll.length) slots for ACP, so many Elliott layers
    // could leave zero room and hide channel / formation lines entirely.
    let inner = [...acp, ...eAll].slice(0, cap);
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
    if (!lastChannelScan?.matched || !lastChannelScan.outcome) return [];
    if ((multiOverlay?.pivotLabels?.length ?? 0) > 0) return [];
    const scanBars = channelScanOverlayBars;
    if (!scanBars?.length) return [];
    return buildChannelScanPivotMarkers(scanBars, lastChannelScan.outcome.pivots, theme);
  }, [lastChannelScan, channelScanOverlayBars, theme, multiOverlay?.pivotLabels?.length]);

  const clearChannelScanUi = useCallback(() => {
    setLastChannelScan(null);
    setLastChannelScanBars(null);
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
    setToolNote(t("app.drawings.cleared"));
    window.setTimeout(() => setToolNote(""), 4000);
  }, [t]);

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
      const rangeCfgPromise = fetchRangeEngineConfig(token).catch(() => null);
      const ingestPromise = fetchEngineSymbolIngestion(token).catch(() => [] as EngineSymbolIngestionApiRow[]);
      const [snaps, syms, sigs, pbal, pfills, liveFills, ds, mc, commDef, rangeCfg, ingest] = await Promise.all([
        fetchEngineSnapshots(token),
        fetchEngineSymbols(token),
        fetchEngineRangeSignals(token, { limit: 80 }),
        fetchPaperBalance(token).catch(() => null),
        fetchPaperFills(token, 15).catch(() => []),
        fetchMyExchangeFills(token, { limit: 30 }).catch(() => []),
        fetchDataSnapshots(token).catch(() => []),
        mcPromise,
        commDefPromise,
        rangeCfgPromise,
        ingestPromise,
      ]);
      setRangeEngineConfig(rangeCfg);
      setEngineSnapshots(snaps);
      setDataSnapshots(ds);
      setMarketContext(mc);
      setEngineSymbols(syms);
      setEngineIngestionRows(ingest);
      setEngineRangeSignals(sigs);
      setPaperBalance(pbal);
      setPaperFills(pfills);
      setExchangeFills(liveFills);
      setCommissionDefaults(commDef);
      setCommissionAccount(null);
      setCommissionAccountErr("");
      setEnginePanelErr("");

      if (rbacIsAdmin) {
        fetchAdminSystemConfig(token, { module: "worker", limit: 200 })
          .then((rows: SystemConfigRowApi[]) => {
            const byKey = new Map<string, unknown>();
            for (const r of rows) byKey.set(`${r.module}.${r.config_key}`, r.value);
            const nansen = byKey.get("worker.nansen_enabled") as any;
            const ext = byKey.get("worker.external_fetch_enabled") as any;
            setWorkerFlags({
              nansenEnabled: typeof nansen?.enabled === "boolean" ? nansen.enabled : true,
              externalFetchEnabled: typeof ext?.enabled === "boolean" ? ext.enabled : true,
            });
          })
          .catch(() => {
            /* ignore */
          });
      }
    } catch (e) {
      setEnginePanelErr(String(e));
    }
  }, [token, barSymbol, barInterval, barExchange, barSegment, rbacIsAdmin]);

  const patchRangeEnginePartial = useCallback(
    async (patch: Record<string, unknown>) => {
      if (!token || !rbacIsOps) return;
      setRangeEnginePatchBusy(true);
      try {
        const next = await patchRangeEngineConfig(token, patch);
        setRangeEngineConfig(next);
        setEnginePanelErr("");
      } catch (e) {
        setEnginePanelErr(String(e));
      } finally {
        setRangeEnginePatchBusy(false);
      }
    },
    [token, rbacIsOps],
  );

  const saveTradingRangeParamsFromDraft = useCallback(async () => {
    const tp: Record<string, unknown> = {
      require_range_regime: trParamsDraft.require_range_regime,
    };
    const numOrNull = (s: string): number | null => {
      const x = s.trim();
      if (x === "") return null;
      const n = parseInt(x, 10);
      return Number.isFinite(n) ? n : null;
    };
    tp.lookback = numOrNull(trParamsDraft.lookback);
    tp.atr_period = numOrNull(trParamsDraft.atr_period);
    tp.atr_sma_period = numOrNull(trParamsDraft.atr_sma_period);
    await patchRangeEnginePartial({ trading_range_params: tp });
  }, [trParamsDraft, patchRangeEnginePartial]);

  const loadCommissionAccount = useCallback(async () => {
    if (!token) return;
    const sym = barSymbol.trim().toUpperCase();
    if (!sym) {
      setCommissionAccountErr(t("app.commission.symbolRequired"));
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
  }, [token, barSymbol, barSegment, t]);

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
      setEngineIngestionRows([]);
      setPaperBalance(null);
      setPaperFills([]);
      setExchangeFills([]);
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

  /** Trading Range sekmesi + otomatik yenileme kutusu: motor anlık görüntüleri ve aralık olayları periyodik çekilir. */
  useEffect(() => {
    if (!drawerOpen || drawerTab !== "trading_range" || !token || !tradingRangeAutoRefresh) return;
    void refreshEnginePanel();
    const id = window.setInterval(() => {
      void refreshEnginePanel();
    }, TRADING_RANGE_DRAWER_REFRESH_MS);
    return () => window.clearInterval(id);
  }, [drawerOpen, drawerTab, token, tradingRangeAutoRefresh, refreshEnginePanel]);

  /** Veri girişi alt sekmesi: kayıtlı engine_symbols listesini hemen doldur (Motor çekmecesine gitmeden). */
  useEffect(() => {
    if (!drawerOpen || drawerTab !== "trading_range" || !token) return;
    if (tradingRangeSubtab !== "data_entry") return;
    void refreshEnginePanel();
  }, [drawerOpen, drawerTab, tradingRangeSubtab, token, refreshEnginePanel]);

  /** Sinyal panosu sekmesi + otomatik yenileme kutusu: `signal_dashboard` anlık görüntüsü periyodik çekilir. */
  useEffect(() => {
    if (!drawerOpen || drawerTab !== "signal_dashboard" || !token || !signalDashboardAutoRefresh) return;
    void refreshEnginePanel();
    const id = window.setInterval(() => {
      void refreshEnginePanel();
    }, SIGNAL_DASHBOARD_DRAWER_REFRESH_MS);
    return () => window.clearInterval(id);
  }, [drawerOpen, drawerTab, token, signalDashboardAutoRefresh, refreshEnginePanel]);

  /**
   * OHLC kaynağı: `ohlcFromBinance` ise Binance spot/FAPI REST (güncel mum); aksi halde JWT + `market_bars`.
   * Otomatik modda giriş + binance + (spot veya futures) → REST; diğer borsalar/segment → DB.
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
        setBarsError(t("app.bars.databaseRequiresAuth"));
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
      setBackfillNote(
        t("app.backfill.done", { upserted: res.upserted, source: res.source ?? "rest" }),
      );
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
        setConfigPreview(t("app.config.adminOnlyPreview"));
      }
      await refreshElliottConfig();
      await refreshAcpConfig();
    } catch (e) {
      setError(String(e));
    } finally {
      setConfigLoading(false);
    }
  }, [t, token, authSession, refreshElliottConfig, refreshAcpConfig]);

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
        description: t("app.acp.dbDescription"),
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
        description: t("app.elliottWave.dbDescription"),
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
    let creds = webOauthBootstrap;
    if (!creds?.clientSecret) {
      try {
        const b = await fetchWebOAuthBootstrap();
        creds = {
          clientId: b.clientId,
          clientSecret: b.clientSecret,
          suggestedLoginEmail: b.suggestedLoginEmail,
        };
        setWebOauthBootstrap(creds);
        setLoginEmail((prev) => (prev.trim() ? prev : b.suggestedLoginEmail || ""));
      } catch (e) {
        setError(String(e));
        return;
      }
    }
    const env = {
      clientId: creds!.clientId || import.meta.env.VITE_OAUTH_CLIENT_ID || "",
      clientSecret: creds!.clientSecret || import.meta.env.VITE_OAUTH_CLIENT_SECRET || "",
      email:
        loginEmail.trim() ||
        import.meta.env.VITE_DEV_EMAIL ||
        creds!.suggestedLoginEmail ||
        "",
      password: loginPassword || import.meta.env.VITE_DEV_PASSWORD || "",
    };
    if (!env.clientId || !env.clientSecret || !env.email || !env.password) {
      setError(t("app.dev.missingWebEnv"));
      return;
    }
    try {
      const tok = await oauthTokenPassword(env);
      setToken(tok.access_token);
      try {
        if (typeof window !== "undefined") {
          const rt = tok.refresh_token?.trim() ?? "";
          if (rt) localStorage.setItem(REFRESH_TOKEN_STORAGE_KEY, rt);
          else localStorage.removeItem(REFRESH_TOKEN_STORAGE_KEY);
          localStorage.setItem(
            ACCESS_TOKEN_EXP_MS_STORAGE_KEY,
            String(Date.now() + Math.max(0, (tok.expires_in ?? 0) * 1000)),
          );
        }
      } catch {
        /* private mode, quota */
      }
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

  const drawerLoggedIn = Boolean(token?.trim());
  useEffect(() => {
    if (!drawerLoggedIn && drawerTab !== "general") {
      setDrawerTab("general");
    }
  }, [drawerLoggedIn, drawerTab]);

  const jumpToHelp = useCallback((topicId: string) => {
    setDrawerOpen(true);
    setDrawerTab("help");
    setHelpFocusId(topicId);
  }, []);

  return (
    <div className="tv-root">
      <header className="tv-topstrip">
        <button
          type="button"
          className="tv-hamburger"
          aria-label={t("app.chartToolbar.menuAria")}
          aria-expanded={drawerOpen}
          aria-controls="qtss-drawer"
          onClick={() => setDrawerOpen(true)}
        >
          <span className="tv-hamburger__bar" />
          <span className="tv-hamburger__bar" />
          <span className="tv-hamburger__bar" />
        </button>
        <div className="tv-topstrip__controls" aria-label={t("app.chartToolbar.symbolStripAria")}>
          <datalist id="qtss-chart-symbol-datalist">
            {symbolDatalist.map((s) => (
              <option key={s} value={s} />
            ))}
          </datalist>
          <input
            className="tv-topstrip__input mono"
            aria-label={t("app.chartToolbar.symbolAria")}
            list={token?.trim() ? "qtss-chart-symbol-datalist" : undefined}
            value={barSymbol}
            onChange={(e) => setBarSymbol(e.target.value.toUpperCase())}
            placeholder="BTCUSDT"
            maxLength={32}
            title={token?.trim() ? t("app.chartToolbar.symbolSuggestTitle") : undefined}
          />
          <select
            className="tv-topstrip__select"
            aria-label={t("app.chartToolbar.intervalAria")}
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
            aria-label={t("app.chartToolbar.ohlcSourceAria")}
            value={chartOhlcMode}
            title={t("app.chartToolbar.ohlcSourceTitle")}
            onChange={(e) => {
              const v = e.target.value as ChartOhlcMode;
              setChartOhlcMode(v);
              persistChartOhlcMode(v);
            }}
          >
            <option value="auto">{t("app.chartToolbar.ohlcAuto")}</option>
            <option value="exchange">{t("app.chartToolbar.ohlcExchange")}</option>
            <option value="database">{t("app.chartToolbar.ohlcDatabase")}</option>
          </select>
          {token?.trim() ? (
            <select
              className="tv-topstrip__select mono"
              aria-label={t("app.chartToolbar.exchangeAria")}
              title={t("app.chartToolbar.exchangeBadgeTitle")}
              value={(barExchange.trim() || "binance").toLowerCase()}
              onChange={(e) => setBarExchange(e.target.value.toLowerCase())}
            >
              {catalogExchanges.length === 0 ? (
                <option value={(barExchange.trim() || "binance").toLowerCase()}>
                  {barExchange.trim() || "binance"}
                </option>
              ) : (
                <>
                  {catalogExchanges.map((ex) => (
                    <option key={ex.id} value={ex.code.trim().toLowerCase()}>
                      {ex.display_name?.trim() || ex.code}
                    </option>
                  ))}
                  {!catalogExchanges.some(
                    (ex) => ex.code.trim().toLowerCase() === (barExchange.trim() || "binance").toLowerCase(),
                  ) ? (
                    <option value={(barExchange.trim() || "binance").toLowerCase()}>
                      {barExchange.trim() || "binance"}
                    </option>
                  ) : null}
                </>
              )}
            </select>
          ) : (
            <span className="tv-topstrip__exchange-tag mono muted" title={t("app.chartToolbar.exchangeBadgeTitle")}>
              {(barExchange.trim() || "binance").toLowerCase()}
            </span>
          )}
          <select
            className="tv-topstrip__select"
            aria-label={t("app.chartToolbar.marketAria")}
            title={t("app.chartToolbar.marketTitle")}
            value={chartToolbarSegmentSelectValue(barSegment)}
            onChange={(e) => setBarSegment(e.target.value === "futures" ? "futures" : "spot")}
          >
            <option value="spot">{t("app.chartToolbar.marketSpot")}</option>
            <option value="futures">{t("app.chartToolbar.marketUsdm")}</option>
          </select>
        </div>
        <div className="tv-topstrip__symbol">
          <span className="muted">
            {ohlcFromBinance ? (
              <>
                {t("app.chartToolbar.sourceBinanceLive")}
                {" · "}
                {normalizeMarketSegment(barSegment) === "futures"
                  ? t("app.chartToolbar.marketUsdm")
                  : t("app.chartToolbar.marketSpot")}
                {binanceKlinesUsesQtssApi(token)
                  ? ` · ${t("app.chartToolbar.sourceBinanceViaBackend")}`
                  : null}
              </>
            ) : token ? (
              t("app.chartToolbar.sourceDbLine", {
                exchange: barExchange,
                segment: barSegment || "spot",
              })
            ) : (
              t("app.chartToolbar.sourceDbNeedLogin")
            )}
          </span>
          {bars && bars.length > 0 ? (
            <span className="muted">{t("app.chartToolbar.barCount", { count: bars.length })}</span>
          ) : null}
          {channelScanSummary ? (
            <span
              className="tv-topstrip__scan"
              title={channelScanHoverTitle || t("app.channelScan.defaultHoverTitle")}
            >
              {channelScanLoading ? t("app.channelScan.scanning") : channelScanSummary}
            </span>
          ) : null}
          {elliottProjectionStatus ? (
            <span
              className="tv-topstrip__scan"
              title="Elliott projeksiyon durumu (invalid/timeout)."
            >
              {elliottProjectionStatus}
            </span>
          ) : null}
          {barsError ? <span className="err tv-topstrip__err" title={barsError}>{barsError.slice(0, 72)}{barsError.length > 72 ? "…" : ""}</span> : null}
          {toolNote ? <span className="muted">{toolNote}</span> : null}
        </div>
        <div className="tv-topstrip__actions">
          <button
            type="button"
            className="theme-toggle"
            title={`ACP bar-ratio: ${acpBarRatioModeLabel} (click to cycle)`}
            onClick={() => {
              cycleAcpBarRatioMode();
              if (bars?.length) runChannelSixScan();
            }}
          >
            BR: {acpBarRatioModeLabel}
          </button>
          <button type="button" className="theme-toggle" onClick={toggleTheme}>
            {theme === "dark" ? t("app.chartToolbar.themeLight") : t("app.chartToolbar.themeDark")}
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
          <aside
            id="qtss-drawer"
            className="tv-drawer"
            aria-modal="true"
            role="dialog"
            aria-label={t("app.drawerPanel.dialogAria")}
          >
            <div className="tv-drawer__head">
              <span>QTSS</span>
              <button
                type="button"
                className="tv-icon-btn"
                onClick={() => setDrawerOpen(false)}
                aria-label={t("app.drawerPanel.closeAria")}
              >
                ×
              </button>
            </div>
            <div className="tv-drawer__body">
              {drawerLoggedIn ? (
                <div className="tv-settings__quick-search">
                  <input
                    className="tv-topstrip__input"
                    value={drawerSearch}
                    onChange={(e) => setDrawerSearch(e.target.value)}
                    placeholder={t("app.drawerPanel.searchPlaceholder")}
                    aria-label={t("app.drawerPanel.searchInputAria")}
                  />
                </div>
              ) : null}
              {drawerLoggedIn ? (
              <div className="tv-settings__tabs" role="tablist" aria-label={t("app.drawerPanel.tablistAria")}>
                <button
                  type="button"
                  role="tab"
                  aria-selected={drawerTab === "general"}
                  className={`tv-settings__tab ${drawerTab === "general" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("general")}
                >
                  {t("drawer.general")}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={drawerTab === "dashboard"}
                  className={`tv-settings__tab ${drawerTab === "dashboard" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("dashboard")}
                >
                  {t("drawer.dashboard")}
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
                  aria-selected={drawerTab === "trading_range"}
                  className={`tv-settings__tab ${drawerTab === "trading_range" ? "is-active" : ""}`}
                  onClick={() => {
                    setDrawerTab("trading_range");
                    setTradingRangeSubtab("main");
                  }}
                >
                  {t("drawer.tradingRange")}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={drawerTab === "signal_dashboard"}
                  className={`tv-settings__tab ${drawerTab === "signal_dashboard" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("signal_dashboard")}
                >
                  {t("drawer.signalDashboard")}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={drawerTab === "orders"}
                  className={`tv-settings__tab ${drawerTab === "orders" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("orders")}
                >
                  Emirler
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={drawerTab === "commission"}
                  className={`tv-settings__tab ${drawerTab === "commission" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("commission")}
                >
                  {t("drawer.commission")}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={drawerTab === "backtest"}
                  className={`tv-settings__tab ${drawerTab === "backtest" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("backtest")}
                >
                  Backtest
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
                  {t("drawer.queues")}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={drawerTab === "notify"}
                  className={`tv-settings__tab ${drawerTab === "notify" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("notify")}
                >
                  Bildirimler
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={drawerTab === "setting"}
                  className={`tv-settings__tab ${drawerTab === "setting" ? "is-active" : ""}`}
                  onClick={() => setDrawerTab("setting")}
                >
                  {t("drawer.setting")}
                </button>
              </div>
              ) : null}
              {drawerLoggedIn && isElliottDrawerGroup ? (
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
                    {t("elliott.summary")}
                  </button>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={drawerTab === "elliott_impulse"}
                    className={`tv-settings__tab ${drawerTab === "elliott_impulse" ? "is-active" : ""}`}
                    onClick={() => setDrawerTab("elliott_impulse")}
                  >
                    {t("elliott.impulse")}
                  </button>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={drawerTab === "elliott_corrective"}
                    className={`tv-settings__tab ${drawerTab === "elliott_corrective" ? "is-active" : ""}`}
                    onClick={() => setDrawerTab("elliott_corrective")}
                  >
                    {t("elliott.corrective")}
                  </button>
                </div>
              ) : null}

              {drawerTab === "general" ? (
                <>
                  {matchesSetting("api sağlık", "health", "durum") ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">{t("drawer.status")}</p>
                      <p className="muted" style={{ margin: 0 }}>
                        {t("drawer.apiHealth")} <span className="mono">{health}</span>
                      </p>
                    </div>
                  ) : null}
                  {matchesSetting("oturum", "config", "giriş", "token", "rol", "rbac") ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">{t("drawer.sessionConfig")}</p>
                      <p className="muted" style={{ fontSize: "0.75rem", marginBottom: "0.35rem" }}>
                        {t("drawer.sessionHint")}
                      </p>
                      <div
                        style={{
                          display: "flex",
                          flexDirection: "column",
                          gap: "0.35rem",
                          marginBottom: "0.35rem",
                        }}
                      >
                        <label className="muted" style={{ fontSize: "0.72rem" }}>
                          {t("drawer.loginEmail")}
                          <input
                            type="email"
                            autoComplete="username"
                            value={loginEmail}
                            onChange={(e) => setLoginEmail(e.target.value)}
                            className="theme-toggle"
                            style={{
                              display: "block",
                              width: "100%",
                              maxWidth: "22rem",
                              marginTop: "0.2rem",
                              padding: "0.25rem 0.4rem",
                            }}
                          />
                        </label>
                        <label className="muted" style={{ fontSize: "0.72rem" }}>
                          {t("drawer.loginPassword")}
                          <input
                            type="password"
                            autoComplete="current-password"
                            value={loginPassword}
                            onChange={(e) => setLoginPassword(e.target.value)}
                            className="theme-toggle"
                            style={{
                              display: "block",
                              width: "100%",
                              maxWidth: "22rem",
                              marginTop: "0.2rem",
                              padding: "0.25rem 0.4rem",
                            }}
                          />
                        </label>
                      </div>
                      <div style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap", alignItems: "center" }}>
                        <LanguageSwitcher
                          accessToken={token}
                          onLocalePatched={(code) => {
                            setAuthSession((prev) =>
                              prev ? { ...prev, preferredLocale: code } : prev,
                            );
                          }}
                        />
                        <button type="button" className="theme-toggle" onClick={tryDevLogin}>
                          {t("drawer.loginTry")}
                        </button>
                        <button type="button" className="theme-toggle" onClick={refreshConfig} disabled={!token || configLoading}>
                          {configLoading ? t("drawer.configLoading") : t("drawer.configRefresh")}
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
                          {t("drawer.logout")}
                        </button>
                      </div>
                      {authMeLoading ? (
                        <p className="muted" style={{ marginTop: "0.35rem" }}>
                          {t("drawer.rolesLoading")}
                        </p>
                      ) : null}
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
                      <p className="tv-drawer__section-head">{t("drawer.appearance")}</p>
                      <button type="button" className="theme-toggle" onClick={toggleTheme}>
                        {theme === "dark" ? t("drawer.themeToggleDark") : t("drawer.themeToggleLight")}
                      </button>
                    </div>
                  ) : null}
                </>
              ) : null}

              {drawerTab === "dashboard" ? <TradeDashboardPanel accessToken={token} /> : null}

              {drawerTab === "backtest" ? (
                <BacktestRunCard
                  accessToken={token}
                  allowBackfill={rbacIsOps}
                  defaultExchange={barExchange}
                  defaultSegment={barSegment || "spot"}
                  defaultSymbol={barSymbol}
                  defaultInterval={barInterval}
                />
              ) : null}

              {drawerTab === "orders" ? <OrdersAndFillsCard accessToken={token} /> : null}

              {drawerTab === "commission" ? (
                <>
                  {matchesSetting(
                    "komisyon",
                    "commission",
                    "ücret",
                    "fee",
                    "maker",
                    "taker",
                    "bps",
                    "binance",
                    "borsa",
                    "oran",
                  ) ? (
                    token ? (
                      <div className="card">
                        <p className="tv-drawer__section-head">{t("app.commissionDrawer.multiExchangeHead")}</p>
                        <p className="muted" style={{ fontSize: "0.78rem", marginBottom: "0.5rem" }}>
                          {t("app.commissionDrawer.scopeIntro")}
                        </p>
                        <p className="tv-drawer__section-head" style={{ marginBottom: "0.35rem" }}>
                          {t("app.commissionDrawer.contextHead")}
                        </p>
                        <p className="muted" style={{ fontSize: "0.72rem", marginBottom: "0.45rem" }}>
                          {t("app.commissionDrawer.contextLead")}{" "}
                          <span className="mono">
                            {barExchange.trim() || "—"}/{normalizeMarketSegment(barSegment)}/{barSymbol.trim() || "—"}/
                            {barInterval.trim() || "—"}
                          </span>
                          {t("app.commissionDrawer.contextTrail")}
                        </p>
                        <p className="tv-drawer__section-head" style={{ marginBottom: "0.3rem" }}>
                          {t("app.commissionDrawer.binanceHead")}
                        </p>
                        <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.35rem" }}>
                          {t("app.commissionDrawer.binanceBody")}
                        </p>
                        {barExchange.trim().toLowerCase() !== "binance" ? (
                          <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.4rem", lineHeight: 1.4 }}>
                            {t("app.commissionDrawer.binanceMismatch")}
                          </p>
                        ) : null}
                        <table
                          style={{
                            width: "100%",
                            fontSize: "0.72rem",
                            borderCollapse: "collapse",
                            marginBottom: "0.35rem",
                          }}
                        >
                          <tbody>
                            <tr>
                              <td
                                className="muted"
                                style={{ padding: "0.1rem 0.35rem 0.1rem 0", verticalAlign: "top" }}
                              >
                                {t("app.paperDrawer.defaultBps")}
                              </td>
                              <td className="mono" style={{ padding: "0.1rem 0", wordBreak: "break-all" }}>
                                {commissionDefaults ? (
                                  <>
                                    maker {commissionDefaults.defaults_bps.maker_bps.toFixed(2)} · taker{" "}
                                    {commissionDefaults.defaults_bps.taker_bps.toFixed(2)}
                                    <br />
                                    <span style={{ opacity: 0.88 }}>
                                      {commissionDefaults.segment}
                                      {commissionDefaults.query_symbol
                                        ? ` · ${commissionDefaults.query_symbol}`
                                        : ""}{" "}
                                      · {commissionDefaults.source}
                                    </span>
                                  </>
                                ) : (
                                  "—"
                                )}
                              </td>
                            </tr>
                            <tr>
                              <td
                                className="muted"
                                style={{ padding: "0.1rem 0.35rem 0.1rem 0", verticalAlign: "top" }}
                              >
                                {t("app.paperDrawer.accountFraction")}
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
                          style={{ fontSize: "0.74rem" }}
                          disabled={commissionAccountBusy}
                          onClick={() => void loadCommissionAccount()}
                        >
                          {commissionAccountBusy
                            ? t("app.commission.fetchBusy")
                            : t("app.commission.fetchLabel")}
                        </button>
                      </div>
                    ) : (
                      <p className="muted">{t("app.commissionDrawer.signInPrompt")}</p>
                    )
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
                          düzeltme segmentleri için geçerlidir. «Gelecek Dalga (Tahmin)» yalnızca o anki grafik
                          periyodunun sütunu için çizilir (ör. 1H mumda 1H kutusu).
                        </p>
                        <div className="tv-elliott-panel__row" style={{ marginBottom: "0.35rem" }}>
                          <label className="tv-elliott-panel__toggle">
                            <input
                              type="checkbox"
                              checked={elliottConfig.enabled}
                              onChange={(e) =>
                                setElliottConfig((c) => ({ ...c, enabled: e.target.checked }))
                              }
                            />
                            <span>Elliott analizi (grafik)</span>
                          </label>
                          <span className="muted" style={{ fontSize: "0.75rem" }}>
                            {elliottConfig.enabled ? `${elliottPanelSwingPivotCount} swing pivot` : ""}
                          </span>
                        </div>
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
                            {ELLIOTT_PATTERN_MENU_ROWS.map((row) =>
                              row.type === "label" ? (
                                <tr key={row.id}>
                                  <td
                                    colSpan={4}
                                    style={{
                                      padding: "0.28rem 0.2rem",
                                      paddingLeft: `${0.35 + row.depth * 0.65}rem`,
                                      fontWeight: row.depth === 0 ? 700 : 650,
                                      fontSize: row.depth === 0 ? "0.82rem" : "0.78rem",
                                    }}
                                  >
                                    {row.titleTr}
                                    <span className="muted" style={{ marginLeft: "0.35rem", fontWeight: 500 }}>
                                      {row.titleEn}
                                    </span>
                                  </td>
                                </tr>
                              ) : (
                                <tr key={row.id}>
                                  <td style={{ padding: "0.2rem", paddingLeft: `${0.45 + row.depth * 0.65}rem` }}>
                                    <span style={{ fontWeight: 600 }}>{row.titleTr}</span>
                                    {row.structure ? (
                                      <span
                                        className="mono muted"
                                        style={{ fontSize: "0.68rem", marginLeft: "0.25rem" }}
                                      >
                                        {row.structure}
                                      </span>
                                    ) : null}
                                  </td>
                                  <td style={{ textAlign: "center" }}>
                                    <input
                                      type="checkbox"
                                      checked={elliottConfig.pattern_menu_by_tf["4h"][row.id]}
                                      onChange={(e) =>
                                        setElliottConfig((c) => patchPatternMenuTf(c, "4h", row.id, e.target.checked))
                                      }
                                    />
                                  </td>
                                  <td style={{ textAlign: "center" }}>
                                    <input
                                      type="checkbox"
                                      checked={elliottConfig.pattern_menu_by_tf["1h"][row.id]}
                                      onChange={(e) =>
                                        setElliottConfig((c) => patchPatternMenuTf(c, "1h", row.id, e.target.checked))
                                      }
                                    />
                                  </td>
                                  <td style={{ textAlign: "center" }}>
                                    <input
                                      type="checkbox"
                                      checked={elliottConfig.pattern_menu_by_tf["15m"][row.id]}
                                      onChange={(e) =>
                                        setElliottConfig((c) => patchPatternMenuTf(c, "15m", row.id, e.target.checked))
                                      }
                                    />
                                  </td>
                                </tr>
                              ),
                            )}
                            <tr>
                              <td style={{ padding: "0.2rem" }}>
                                <span style={{ fontWeight: 600 }}>Gelecek Dalga (Tahmin)</span>
                                <span className="muted" style={{ marginLeft: "0.35rem" }}>
                                  — İleri Fib projeksiyon
                                </span>
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="checkbox"
                                  checked={elliottConfig.show_projection_4h}
                                  disabled={!elliottConfig.enabled || !elliottConfig.pattern_menu_by_tf["4h"].motive_impulse}
                                  onChange={(e) =>
                                    setElliottConfig((c) => ({ ...c, show_projection_4h: e.target.checked }))
                                  }
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="checkbox"
                                  checked={elliottConfig.show_projection_1h}
                                  disabled={!elliottConfig.enabled || !elliottConfig.pattern_menu_by_tf["1h"].motive_impulse}
                                  onChange={(e) =>
                                    setElliottConfig((c) => ({ ...c, show_projection_1h: e.target.checked }))
                                  }
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="checkbox"
                                  checked={elliottConfig.show_projection_15m}
                                  disabled={!elliottConfig.enabled || !elliottConfig.pattern_menu_by_tf["15m"].motive_impulse}
                                  onChange={(e) =>
                                    setElliottConfig((c) => ({ ...c, show_projection_15m: e.target.checked }))
                                  }
                                />
                              </td>
                            </tr>
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
                                  className="tv-elliott-color-swatch"
                                  value={elliottColorInputValue(elliottConfig.mtf_zigzag_color_4h)}
                                  onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_zigzag_color_4h: e.target.value }))}
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="color"
                                  className="tv-elliott-color-swatch"
                                  value={elliottColorInputValue(elliottConfig.mtf_zigzag_color_1h)}
                                  onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_zigzag_color_1h: e.target.value }))}
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="color"
                                  className="tv-elliott-color-swatch"
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
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="color"
                                  className="tv-elliott-color-swatch"
                                  value={elliottColorInputValue(elliottConfig.mtf_wave_color_4h)}
                                  onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_wave_color_4h: e.target.value }))}
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="color"
                                  className="tv-elliott-color-swatch"
                                  value={elliottColorInputValue(elliottConfig.mtf_wave_color_1h)}
                                  onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_wave_color_1h: e.target.value }))}
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="color"
                                  className="tv-elliott-color-swatch"
                                  value={elliottColorInputValue(elliottConfig.mtf_wave_color_15m)}
                                  onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_wave_color_15m: e.target.value }))}
                                />
                              </td>
                            </tr>
                            <tr>
                              <td style={{ padding: "0.2rem" }}>Etiket renk</td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="color"
                                  className="tv-elliott-color-swatch"
                                  value={elliottColorInputValue(elliottConfig.mtf_label_color_4h)}
                                  onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_label_color_4h: e.target.value }))}
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="color"
                                  className="tv-elliott-color-swatch"
                                  value={elliottColorInputValue(elliottConfig.mtf_label_color_1h)}
                                  onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_label_color_1h: e.target.value }))}
                                />
                              </td>
                              <td style={{ textAlign: "center" }}>
                                <input
                                  type="color"
                                  className="tv-elliott-color-swatch"
                                  value={elliottColorInputValue(elliottConfig.mtf_label_color_15m)}
                                  onChange={(e) => setElliottConfig((c) => ({ ...c, mtf_label_color_15m: e.target.value }))}
                                />
                              </td>
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
                            hideEnabledToggle
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
                      <p className="tv-drawer__section-head">{t("app.channelScan.drawerSectionTitle")}</p>
                      <p className="muted" style={{ fontSize: "0.8rem", marginBottom: "0.5rem" }}>
                        {t("app.channelScan.drawerIntro")}
                      </p>
                      {channelScanLoading ? <p className="muted">{t("app.channelScan.scanning")}</p> : null}
                      {channelScanError ? <p className="err">{channelScanError}</p> : null}
                      {lastChannelScan?.matched ? <ChannelScanMatchesTable res={lastChannelScan} /> : null}
                      {channelScanSummary ? (
                        <p className="muted" style={{ fontSize: "0.75rem", marginTop: "0.5rem" }}>
                          {t("app.channelScan.drawerFootnote")}
                        </p>
                      ) : null}
                    </div>
                  ) : null}
                </>
              ) : null}

              {drawerTab === "trading_range" ? (
                <>
                  {matchesSetting(
                    "range",
                    "aralık",
                    "trading",
                    "sweep",
                    "süpürme",
                    "grafik",
                    "katman",
                    "işaret",
                    "marker",
                    "pozisyon",
                    "position",
                    "olay",
                    "db",
                    "likidite",
                    "liquidity",
                    "açık",
                    "open",
                    "data",
                    "veri",
                    "giriş",
                    "entry",
                    "backfill",
                    "limit",
                    "symbol",
                    "sembol",
                    "exchange",
                    "segment",
                    "setup",
                    "özet",
                    "işlem",
                    "summary",
                    "bölüm",
                  ) ? (
                    <>
                      <div
                        className="tv-settings__tabs tv-settings__subtabs tv-settings__subtabs--cols-4"
                        role="tablist"
                        aria-label={t("app.tradingRangeDrawer.subtabListAria")}
                      >
                        <button
                          type="button"
                          role="tab"
                          aria-selected={tradingRangeSubtab === "main"}
                          className={`tv-settings__tab ${tradingRangeSubtab === "main" ? "is-active" : ""}`}
                          onClick={() => setTradingRangeSubtab("main")}
                        >
                          {t("app.tradingRangeDrawer.sectionTitle")}
                        </button>
                        <button
                          type="button"
                          role="tab"
                          aria-selected={tradingRangeSubtab === "data_entry"}
                          className={`tv-settings__tab ${tradingRangeSubtab === "data_entry" ? "is-active" : ""}`}
                          onClick={() => setTradingRangeSubtab("data_entry")}
                        >
                          {t("app.tradingRangeDrawer.dataEntryTitle")}
                        </button>
                        <button
                          type="button"
                          role="tab"
                          aria-selected={tradingRangeSubtab === "setup"}
                          className={`tv-settings__tab ${tradingRangeSubtab === "setup" ? "is-active" : ""}`}
                          onClick={() => setTradingRangeSubtab("setup")}
                        >
                          {t("app.tradingRangeEventsSetup.sectionTitle")}
                        </button>
                        <button
                          type="button"
                          role="tab"
                          aria-selected={tradingRangeSubtab === "trade_summary"}
                          className={`tv-settings__tab ${tradingRangeSubtab === "trade_summary" ? "is-active" : ""}`}
                          onClick={() => setTradingRangeSubtab("trade_summary")}
                        >
                          {t("app.tradingRangeSetup.tableTitle")}
                        </button>
                      </div>
                      {tradingRangeSubtab === "data_entry" ? (
                        <div className="card">
                          {enginePanelErr ? <p className="err">{enginePanelErr}</p> : null}
                          <TradingRangeDataEntryPanel
                            accessToken={token}
                            onEnginesUpdated={() => void refreshEnginePanel()}
                            isLoggedIn={!!token}
                            canOps={rbacIsOps}
                            exchange={barExchange}
                            segment={barSegment}
                            symbol={barSymbol}
                            interval={barInterval}
                            limit={barLimit}
                            onExchangeChange={setBarExchange}
                            onSegmentChange={setBarSegment}
                            onSymbolChange={setBarSymbol}
                            onIntervalChange={setBarInterval}
                            onLimitChange={setBarLimit}
                            onApplyChart={() => void loadChartFromToolbar()}
                            applyChartBusy={barsLoading}
                            onBackfillRest={token && rbacIsOps ? () => void backfillFromRest() : undefined}
                            backfillDisabled={backfillLoading || barsLoading || !token}
                            backfillBusy={backfillLoading}
                            backfillNote={backfillNote}
                            engineSymbolDraft={engineFormSymbol}
                            engineIntervalDraft={engineFormInterval}
                            onEngineSymbolDraftChange={setEngineFormSymbol}
                            onEngineIntervalDraftChange={setEngineFormInterval}
                            onSyncScopeToEngineDraft={() => {
                              setEngineFormSymbol(barSymbol.trim());
                              setEngineFormInterval(barInterval.trim());
                            }}
                            onRegisterEngine={async () => {
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
                            engineRegisterBusy={engineFormBusy}
                            registeredTargets={engineSymbols}
                            onEngineTargetsPatchError={(message) => setEnginePanelErr(message)}
                          />
                        </div>
                      ) : null}
                      {tradingRangeSubtab === "main" ? (
                        <div className="card">
                          <p className="tv-drawer__section-head">{t("app.tradingRangeDrawer.sectionTitle")}</p>
                      <p className="muted" style={{ fontSize: "0.78rem", marginBottom: "0.5rem" }}>
                        {t("app.tradingRangeDrawer.intro")}
                      </p>
                      {token ? (
                        <>
                          <label
                            className="muted tv-elliott-panel__field tv-elliott-panel__field--check"
                            style={{ display: "flex", alignItems: "flex-start", gap: "0.45rem", marginBottom: "0.35rem" }}
                          >
                            <input
                              type="checkbox"
                              checked={tradingRangeAutoRefresh}
                              onChange={(e) => setTradingRangeAutoRefresh(e.target.checked)}
                            />
                            <span style={{ fontSize: "0.72rem" }}>{t("app.tradingRangeDrawer.autoRefreshCheckbox")}</span>
                          </label>
                          {tradingRangeAutoRefresh ? (
                            <p className="muted" style={{ fontSize: "0.72rem", marginBottom: "0.4rem" }}>
                              {t("app.tradingRangeDrawer.autoRefreshActive", {
                                seconds: TRADING_RANGE_DRAWER_REFRESH_MS / 1000,
                              })}
                            </p>
                          ) : (
                            <p className="muted" style={{ fontSize: "0.72rem", marginBottom: "0.4rem" }}>
                              {t("app.tradingRangeDrawer.autoRefreshHint")}
                            </p>
                          )}
                          <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.45rem", lineHeight: 1.45 }}>
                            {t("app.tradingRangeDrawer.chartScopeHint")}
                          </p>
                        </>
                      ) : null}
                      <div className="tv-trading-range-checks">
                        <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
                          <input
                            type="checkbox"
                            checked={showDbTradingRangeLayer}
                            onChange={(e) => setShowDbTradingRangeLayer(e.target.checked)}
                          />
                          <span>{t("app.engineDrawer.chkTradingRange")}</span>
                        </label>
                        <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
                          <input
                            type="checkbox"
                            checked={showDbSweepMarkers}
                            onChange={(e) => setShowDbSweepMarkers(e.target.checked)}
                          />
                          <span>{t("app.engineDrawer.chkSweepMarkers")}</span>
                        </label>
                        <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
                          <input
                            type="checkbox"
                            checked={showDbRangeSignalMarkers}
                            onChange={(e) => setShowDbRangeSignalMarkers(e.target.checked)}
                          />
                          <span>{t("app.engineDrawer.chkRangeSignals")}</span>
                        </label>
                        <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
                          <input
                            type="checkbox"
                            checked={showDbOpenPositionLine}
                            onChange={(e) => setShowDbOpenPositionLine(e.target.checked)}
                          />
                          <span>{t("app.engineDrawer.chkOpenPositionLine")}</span>
                        </label>
                      </div>
                      {tradingRangeScorePayload ? (
                        <div
                          style={{
                            marginTop: "0.45rem",
                            padding: "0.45rem 0.5rem",
                            borderRadius: "8px",
                            border: "1px solid var(--tv-border, rgba(255,255,255,0.08))",
                            background: "color-mix(in srgb, var(--card) 86%, transparent)",
                          }}
                        >
                          <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: "0.5rem" }}>
                            <p className="muted" style={{ margin: 0, fontSize: "0.78rem", fontWeight: 700 }}>
                              Score
                            </p>
                            <p className="mono" style={{ margin: 0, fontSize: "0.78rem" }}>
                              {String(tradingRangeScorePayload.setup_side ?? "—")} ·{" "}
                              {typeof tradingRangeScorePayload.setup_score_best === "number"
                                ? tradingRangeScorePayload.setup_score_best
                                : "—"}
                              /100
                            </p>
                          </div>
                          <p className="muted" style={{ margin: "0.25rem 0 0", fontSize: "0.7rem", lineHeight: 1.45 }}>
                            Guardrails:{" "}
                            <span className="mono">
                              {tradingRangeScorePayload.guardrails_pass ? "PASS" : "REJECT"}
                            </span>
                            {" · "}Touches:{" "}
                            <span className="mono">
                              {typeof tradingRangeScorePayload.support_touches === "number"
                                ? tradingRangeScorePayload.support_touches
                                : "—"}
                              /
                              {typeof tradingRangeScorePayload.resistance_touches === "number"
                                ? tradingRangeScorePayload.resistance_touches
                                : "—"}
                            </span>
                            {" · "}Close breakout:{" "}
                            <span className="mono">
                              {tradingRangeScorePayload.close_breakout ? "yes" : "no"}
                            </span>
                            {" · "}Zone:{" "}
                            <span className="mono">{String(tradingRangeScorePayload.range_zone ?? "—")}</span>
                          </p>
                          <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: "0.25rem 0.5rem", marginTop: "0.35rem" }}>
                            <div className="muted" style={{ fontSize: "0.7rem" }}>
                              Touch: <span className="mono">{tradingRangeScorePayload.score_touch_long ?? "—"}</span>
                            </div>
                            <div className="muted" style={{ fontSize: "0.7rem" }}>
                              Rejection: <span className="mono">{tradingRangeScorePayload.score_rejection_long ?? "—"}</span>
                            </div>
                            <div className="muted" style={{ fontSize: "0.7rem" }}>
                              Osc: <span className="mono">{tradingRangeScorePayload.score_oscillator_long ?? "—"}</span>
                            </div>
                            <div className="muted" style={{ fontSize: "0.7rem" }}>
                              Breakout: <span className="mono">{tradingRangeScorePayload.score_breakout_long ?? "—"}</span>
                            </div>
                          </div>
                          {tradingRangeScorePayload.volume_unavailable ? (
                            <p className="muted" style={{ margin: "0.3rem 0 0", fontSize: "0.65rem" }}>
                              Volume score: N/A (no volume in OHLC feed)
                            </p>
                          ) : null}
                        </div>
                      ) : null}
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
                        {engineListRefreshing
                          ? t("app.tradingRangeDrawer.refreshBusy")
                          : t("app.tradingRangeDrawer.refreshNow")}
                      </button>
                      {enginePanelErr ? <p className="err">{enginePanelErr}</p> : null}
                      {token ? (
                        <TradingRangeFuturesQuickTrade
                          accessToken={token}
                          exchange={barExchange}
                          segment={barSegment}
                          symbol={barSymbol}
                        />
                      ) : null}
                      {!token ? <p className="muted">{t("app.tradingRangeDrawer.loginHint")}</p> : null}
                        </div>
                      ) : null}
                      {tradingRangeSubtab === "setup" ? (
                        <div className="card">
                          <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.4rem", lineHeight: 1.45 }}>
                            {t("app.tradingRangeEventsSetup.intro")}{" "}
                            <HelpCrossLink topicId="engine-range-signals" onOpen={jumpToHelp} label="SSS" />
                          </p>
                          {token ? (
                            <>
                              <TradingRangeSetupEngineSymbolsPanel
                                engineSymbols={engineSymbols}
                                engineSnapshots={engineSnapshots}
                                toolbarExchange={barExchange}
                                toolbarSegment={barSegment}
                                toolbarSymbol={barSymbol}
                                toolbarInterval={barInterval}
                              />
                              <p
                                className="tv-drawer__section-head"
                                style={{ marginTop: "0.55rem", marginBottom: "0.25rem" }}
                              >
                                {t("app.tradingRangeEventsSetup.eventsSubhead")}
                              </p>
                              <TradingRangeSetupTable
                                events={engineRangeSignals}
                                engineSnapshots={engineSnapshots}
                                engineSymbols={engineSymbols}
                              />
                            </>
                          ) : (
                            <p className="muted">{t("app.tradingRangeDrawer.loginHint")}</p>
                          )}
                        </div>
                      ) : null}
                      {tradingRangeSubtab === "trade_summary" ? (
                        <div className="card">
                          <p className="tv-drawer__section-head" style={{ marginBottom: "0.25rem" }}>
                            {t("app.tradingRangeSetup.tableTitle")}
                          </p>
                          {tradingRangeTakerFraction == null && token ? (
                            <p className="muted" style={{ fontSize: "0.65rem", marginBottom: "0.35rem" }}>
                              {t("app.tradingRangeSetup.feesHintLoadCommission")}
                            </p>
                          ) : null}
                          {token ? (
                            <TradingRangeTradeSummary
                              setups={chartRangeSetups}
                              takerFraction={tradingRangeTakerFraction}
                              engineSnapshots={engineSnapshots}
                              engineSymbols={engineSymbols}
                            />
                          ) : (
                            <p className="muted">{t("app.tradingRangeDrawer.loginHint")}</p>
                          )}
                        </div>
                      ) : null}
                    </>
                  ) : null}
                </>
              ) : null}

              {drawerTab === "signal_dashboard" ? (
                <div className="card">
                  <p className="tv-drawer__section-head">{t("app.signalDashboardDrawer.sectionTitle")}</p>
                  <p className="muted" style={{ fontSize: "0.78rem", marginBottom: "0.5rem" }}>
                    {t("app.signalDashboardDrawer.intro")}
                  </p>
                  {token ? (
                    <>
                      <label
                        className="muted tv-elliott-panel__field tv-elliott-panel__field--check"
                        style={{ display: "flex", alignItems: "flex-start", gap: "0.45rem", marginBottom: "0.35rem" }}
                      >
                        <input
                          type="checkbox"
                          checked={signalDashboardAutoRefresh}
                          onChange={(e) => setSignalDashboardAutoRefresh(e.target.checked)}
                        />
                        <span style={{ fontSize: "0.72rem" }}>{t("app.signalDashboardDrawer.autoRefreshCheckbox")}</span>
                      </label>
                      {signalDashboardAutoRefresh ? (
                        <p className="muted" style={{ fontSize: "0.72rem", marginBottom: "0.4rem" }}>
                          {t("app.signalDashboardDrawer.autoRefreshActive", {
                            seconds: SIGNAL_DASHBOARD_DRAWER_REFRESH_MS / 1000,
                          })}
                        </p>
                      ) : (
                        <p className="muted" style={{ fontSize: "0.72rem", marginBottom: "0.4rem" }}>
                          {t("app.signalDashboardDrawer.autoRefreshHint")}
                        </p>
                      )}
                    </>
                  ) : null}
                  <button
                    type="button"
                    className="theme-toggle"
                    style={{ marginTop: "0.2rem", fontSize: "0.78rem" }}
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
                    {engineListRefreshing
                      ? t("app.signalDashboardDrawer.refreshBusy")
                      : t("app.signalDashboardDrawer.refreshNow")}
                  </button>
                  {enginePanelErr ? <p className="err">{enginePanelErr}</p> : null}
                  {token ? (
                    <div style={{ marginTop: "0.55rem" }}>
                      <SignalDashboardDrawerPanel
                        snapshots={signalDashboardSnapshots}
                        chartMatchedEngineSymbolId={dbSignalDashboardSnapshot?.engine_symbol_id ?? null}
                      />
                    </div>
                  ) : (
                    <p className="muted" style={{ marginTop: "0.45rem" }}>
                      {t("app.signalDashboardDrawer.loginHint")}
                    </p>
                  )}
                </div>
              ) : null}

              {drawerTab === "engine" ? (
                <>
                  {matchesSetting("motor", "engine", "snapshot", "sembol", "worker") ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">{t("app.engineDrawer.sectionTitle")}</p>
                      <p className="muted" style={{ fontSize: "0.78rem", marginBottom: "0.5rem" }}>
                        {t("app.engineDrawer.intro")}
                      </p>
                      <ul className="muted" style={{ fontSize: "0.72rem", margin: "0 0 0.55rem 1rem", lineHeight: 1.45 }}>
                        <li>{t("app.engineDrawer.liData")}</li>
                        <li>{t("app.engineDrawer.liWorker")}</li>
                        <li>{t("app.engineDrawer.liChartMapping")}</li>
                        <li>{t("app.engineDrawer.liPaper")}</li>
                        <li>{t("app.engineDrawer.liConfluence")}</li>
                      </ul>
                      {token ? (
                        <div
                          className="muted"
                          style={{
                            marginTop: "0.5rem",
                            padding: "0.45rem 0.5rem",
                            border: "1px solid color-mix(in srgb, var(--fg, #ccc) 18%, transparent)",
                            borderRadius: 6,
                            fontSize: "0.72rem",
                          }}
                        >
                          <p className="tv-drawer__section-head" style={{ marginBottom: "0.35rem" }}>
                            {t("app.engineDrawer.rangeConfigHead")}
                          </p>
                          <p style={{ marginBottom: "0.45rem", lineHeight: 1.45 }}>
                            {t("app.engineDrawer.rangeConfigIntro")}
                          </p>
                          {rangeEngineConfig?.worker?.refresh_requested ? (
                            <p style={{ marginBottom: "0.4rem", opacity: 0.95 }}>
                              {t("app.engineDrawer.workerRefreshPending")}
                            </p>
                          ) : null}
                          {rbacIsOps ? (
                            <>
                              <p style={{ marginBottom: "0.3rem", fontWeight: 600 }}>
                                {t("app.engineDrawer.executionGatesHead")}
                              </p>
                              <div
                                style={{
                                  display: "flex",
                                  flexDirection: "column",
                                  gap: "0.3rem",
                                  marginBottom: "0.45rem",
                                }}
                              >
                                <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
                                  <input
                                    type="checkbox"
                                    checked={rangeEngineConfig?.execution_gates?.allow_long_open !== false}
                                    disabled={rangeEnginePatchBusy}
                                    onChange={(e) =>
                                      void patchRangeEnginePartial({
                                        execution_gates: { allow_long_open: e.target.checked },
                                      })
                                    }
                                  />
                                  <span>{t("app.engineDrawer.allowLongOpen")}</span>
                                </label>
                                <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
                                  <input
                                    type="checkbox"
                                    checked={rangeEngineConfig?.execution_gates?.allow_short_open !== false}
                                    disabled={rangeEnginePatchBusy}
                                    onChange={(e) =>
                                      void patchRangeEnginePartial({
                                        execution_gates: { allow_short_open: e.target.checked },
                                      })
                                    }
                                  />
                                  <span>{t("app.engineDrawer.allowShortOpen")}</span>
                                </label>
                                <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
                                  <input
                                    type="checkbox"
                                    checked={rangeEngineConfig?.execution_gates?.allow_all_closes !== false}
                                    disabled={rangeEnginePatchBusy}
                                    onChange={(e) =>
                                      void patchRangeEnginePartial({
                                        execution_gates: { allow_all_closes: e.target.checked },
                                      })
                                    }
                                  />
                                  <span>{t("app.engineDrawer.allowAllCloses")}</span>
                                </label>
                              </div>
                              <button
                                type="button"
                                className="theme-toggle"
                                style={{ fontSize: "0.75rem", marginBottom: "0.45rem" }}
                                disabled={rangeEnginePatchBusy}
                                onClick={() =>
                                  void patchRangeEnginePartial({ worker: { refresh_requested: true } })
                                }
                              >
                                {rangeEnginePatchBusy
                                  ? t("app.engineDrawer.workerRefreshBusy")
                                  : t("app.engineDrawer.workerRefreshRequest")}
                              </button>
                              <p className="tv-drawer__section-head" style={{ marginTop: "0.45rem", marginBottom: "0.3rem" }}>
                                {t("app.engineDrawer.trParamsHead")}
                              </p>
                              <p style={{ marginBottom: "0.35rem", lineHeight: 1.45 }}>
                                {t("app.engineDrawer.trParamsHint")}
                              </p>
                              <div className="tv-settings__fields" style={{ marginBottom: "0.35rem" }}>
                                <label>
                                  <span className="muted">{t("app.engineDrawer.trLookback")}</span>
                                  <input
                                    className="mono"
                                    value={trParamsDraft.lookback}
                                    onChange={(e) =>
                                      setTrParamsDraft((d) => ({ ...d, lookback: e.target.value }))
                                    }
                                    placeholder="50"
                                  />
                                </label>
                                <label>
                                  <span className="muted">{t("app.engineDrawer.trAtrPeriod")}</span>
                                  <input
                                    className="mono"
                                    value={trParamsDraft.atr_period}
                                    onChange={(e) =>
                                      setTrParamsDraft((d) => ({ ...d, atr_period: e.target.value }))
                                    }
                                    placeholder="14"
                                  />
                                </label>
                                <label>
                                  <span className="muted">{t("app.engineDrawer.trAtrSmaPeriod")}</span>
                                  <input
                                    className="mono"
                                    value={trParamsDraft.atr_sma_period}
                                    onChange={(e) =>
                                      setTrParamsDraft((d) => ({ ...d, atr_sma_period: e.target.value }))
                                    }
                                    placeholder="50"
                                  />
                                </label>
                              </div>
                              <label className="muted tv-elliott-panel__field tv-elliott-panel__field--check">
                                <input
                                  type="checkbox"
                                  checked={trParamsDraft.require_range_regime}
                                  onChange={(e) =>
                                    setTrParamsDraft((d) => ({
                                      ...d,
                                      require_range_regime: e.target.checked,
                                    }))
                                  }
                                />
                                <span>{t("app.engineDrawer.trRequireRangeRegime")}</span>
                              </label>
                              <button
                                type="button"
                                className="theme-toggle"
                                style={{ marginTop: "0.35rem", fontSize: "0.75rem" }}
                                disabled={rangeEnginePatchBusy}
                                onClick={() => void saveTradingRangeParamsFromDraft()}
                              >
                                {rangeEnginePatchBusy
                                  ? t("app.engineDrawer.saveTrParamsBusy")
                                  : t("app.engineDrawer.saveTrParams")}
                              </button>
                            </>
                          ) : (
                            <p style={{ lineHeight: 1.45 }}>{t("app.engineDrawer.rangeConfigReadOnly")}</p>
                          )}
                        </div>
                      ) : null}
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
                        {engineListRefreshing
                          ? t("app.engineDrawer.refreshBusy")
                          : t("app.engineDrawer.refreshNow")}
                      </button>
                      {enginePanelErr ? <p className="err">{enginePanelErr}</p> : null}
                      {token ? (
                        <>
                          <p className="muted" style={{ marginTop: "0.45rem", fontSize: "0.8rem" }}>
                            {t("app.engineDrawer.addTargetLead")}
                            {rbacIsOps ? null : (
                              <span>
                                {" "}
                                {t("app.engineDrawer.addTargetRbacFull")}
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
                            {t("app.engineDrawer.registeredTargets", { count: engineSymbols.length })}
                          </p>
                          <ul className="tv-drawer-target-list muted mono">
                            {engineSymbols.map((s) => (
                              <li key={s.id} className="tv-drawer-target-list__item">
                                <span className="tv-drawer-target-list__meta">
                                  {s.enabled ? "●" : "○"} {s.exchange}/{s.segment} {s.symbol} {s.interval}
                                  {s.label ? ` — ${s.label}` : ""}
                                </span>
                                {rbacIsOps ? (
                                  <div className="tv-drawer-target-list__actions">
                                    <select
                                      className="mono"
                                      value={(s.signal_direction_mode ?? "auto_segment").toLowerCase()}
                                      title={t("app.engineDrawer.signalModeTitle")}
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
                                      <option value="auto_segment">{t("app.engineDrawer.optAutoSegment")}</option>
                                      <option value="long_only">{t("app.engineDrawer.optLongOnly")}</option>
                                      <option value="both">{t("app.engineDrawer.optBoth")}</option>
                                      <option value="short_only">{t("app.engineDrawer.optShortOnly")}</option>
                                    </select>
                                    <button
                                      type="button"
                                      className="theme-toggle"
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
                                      {s.enabled
                                        ? t("app.engineDrawer.toggleDisable")
                                        : t("app.engineDrawer.toggleEnable")}
                                    </button>
                                  </div>
                                ) : null}
                              </li>
                            ))}
                          </ul>
                          <p className="tv-drawer__section-head" style={{ marginTop: "0.75rem" }}>
                            {t("app.engineDrawer.ingestionHead")}
                          </p>
                          <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.4rem", lineHeight: 1.45 }}>
                            {t("app.engineDrawer.ingestionIntro")}
                          </p>
                          {engineIngestionRows.length === 0 ? (
                            <p className="muted" style={{ fontSize: "0.72rem" }}>
                              {t("app.engineDrawer.ingestionEmpty")}
                            </p>
                          ) : (
                            <div style={{ overflowX: "auto", maxHeight: "14rem", overflowY: "auto" }}>
                              <table
                                className="mono"
                                style={{ width: "100%", fontSize: "0.65rem", borderCollapse: "collapse" }}
                              >
                                <thead>
                                  <tr className="muted">
                                    <th style={{ textAlign: "left", padding: "0.2rem 0.35rem 0.2rem 0" }}>
                                      {t("app.engineDrawer.ingestionColTarget")}
                                    </th>
                                    <th style={{ textAlign: "right", padding: "0.2rem 0.25rem" }}>
                                      {t("app.engineDrawer.ingestionColBars")}
                                    </th>
                                    <th style={{ textAlign: "right", padding: "0.2rem 0.25rem" }}>
                                      {t("app.engineDrawer.ingestionColGaps")}
                                    </th>
                                    <th style={{ textAlign: "left", padding: "0.2rem 0.25rem" }}>
                                      {t("app.engineDrawer.ingestionColMaxOpen")}
                                    </th>
                                    <th style={{ textAlign: "left", padding: "0.2rem 0" }}>
                                      {t("app.engineDrawer.ingestionColError")}
                                    </th>
                                  </tr>
                                </thead>
                                <tbody>
                                  {engineIngestionRows.map((row) => (
                                    <tr key={row.id}>
                                      <td style={{ padding: "0.18rem 0.35rem 0.18rem 0", wordBreak: "break-all" }}>
                                        {row.enabled ? "●" : "○"}{" "}
                                        {row.exchange}/{row.segment} {row.symbol} {row.interval}
                                      </td>
                                      <td style={{ textAlign: "right", padding: "0.18rem 0.25rem" }}>
                                        {row.bar_row_count ?? "—"}
                                      </td>
                                      <td style={{ textAlign: "right", padding: "0.18rem 0.25rem" }}>
                                        {row.gap_count ?? "—"}
                                      </td>
                                      <td style={{ padding: "0.18rem 0.25rem", fontSize: "0.62rem", wordBreak: "break-all" }}>
                                        {row.max_open_time
                                          ? row.max_open_time.slice(0, 19).replace("T", " ")
                                          : "—"}
                                      </td>
                                      <td
                                        style={{
                                          padding: "0.18rem 0",
                                          fontSize: "0.62rem",
                                          wordBreak: "break-word",
                                          color: row.last_error ? "var(--err, #f66)" : undefined,
                                        }}
                                      >
                                        {row.last_error
                                          ? row.last_error
                                          : row.last_backfill_at
                                            ? `↻ ${row.last_backfill_at.slice(0, 19).replace("T", " ")}`
                                            : "—"}
                                      </td>
                                    </tr>
                                  ))}
                                </tbody>
                              </table>
                            </div>
                          )}
                          {matchesSetting(
                            "paper",
                            "dry",
                            "f4",
                            "ozet",
                            "islem",
                            "işlem",
                            "portfolio",
                            "birleşik",
                            "f5",
                          ) ? (
                            <div className="card" style={{ marginTop: "0.65rem", padding: "0.55rem" }}>
                              <p className="tv-drawer__section-head" style={{ marginBottom: "0.35rem" }}>
                                {t("app.paperDrawer.rangePaperHead")}
                              </p>
                              <p className="muted" style={{ fontSize: "0.72rem", marginBottom: "0.4rem" }}>
                                {t("app.paperDrawer.topBarLead")}{" "}
                                <span className="mono">
                                  {barExchange.trim() || "—"}/{normalizeMarketSegment(barSegment)}/{barSymbol.trim() || "—"}/{barInterval.trim() || "—"}
                                </span>
                                {t("app.paperDrawer.topBarTrail")}
                              </p>
                              <p className="muted" style={{ fontSize: "0.68rem", marginBottom: "0.45rem" }}>
                                {t("app.paperDrawer.commissionMovedHint")}{" "}
                                <button
                                  type="button"
                                  className="tv-link-btn"
                                  style={{ fontSize: "inherit" }}
                                  onClick={() => setDrawerTab("commission")}
                                >
                                  {t("drawer.commission")}
                                </button>
                              </p>
                              <table style={{ width: "100%", fontSize: "0.72rem", borderCollapse: "collapse", marginBottom: "0.45rem" }}>
                                <tbody>
                                  <tr>
                                    <td className="muted" style={{ padding: "0.1rem 0.35rem 0.1rem 0", verticalAlign: "top" }}>
                                      {t("app.paperDrawer.motorDbChain")}
                                    </td>
                                    <td className="mono" style={{ padding: "0.1rem 0", wordBreak: "break-all" }}>
                                      {chartDerivedOpenPosition
                                        ? `${chartDerivedOpenPosition.side.toUpperCase()} @ ${chartDerivedOpenPosition.entryPrice.toFixed(4)}`
                                        : t("app.paperDrawer.noOpenSide")}
                                    </td>
                                  </tr>
                                  <tr>
                                    <td className="muted" style={{ padding: "0.1rem 0.35rem 0.1rem 0", verticalAlign: "top" }}>
                                      {t("app.paperDrawer.recentRangeEvents")}
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
                                      {t("app.paperDrawer.paperQuote")}
                                    </td>
                                    <td className="mono" style={{ padding: "0.1rem 0" }}>
                                      {paperBalance
                                        ? String(paperBalance.quote_balance)
                                        : t("app.paperDrawer.paperQuoteEmpty")}
                                    </td>
                                  </tr>
                                  <tr>
                                    <td className="muted" style={{ padding: "0.1rem 0.35rem 0.1rem 0", verticalAlign: "top" }}>
                                      {t("app.paperDrawer.paperBase")}
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
                          {token && rbacIsOps ? (
                            <div className="card">
                              <p className="tv-drawer__section-head">Son canlı dolumlar</p>
                              <p className="muted" style={{ fontSize: "0.68rem", marginTop: 0, marginBottom: "0.25rem" }}>
                                API <code>/api/v1/fills</code> (exchange_fills)
                              </p>
                              <div style={{ maxHeight: "7rem", overflow: "auto", fontSize: "0.65rem" }} className="mono muted">
                                {exchangeFills.length === 0 ? (
                                  <span>—</span>
                                ) : (
                                  exchangeFills.slice(0, 8).map((f) => (
                                    <div key={f.id} style={{ marginBottom: "0.25rem" }}>
                                      {f.exchange}/{f.segment} {f.symbol} oid {String(f.venue_order_id)}{" "}
                                      {f.fill_quantity != null ? `qty ${String(f.fill_quantity)}` : ""}{" "}
                                      {f.fill_price != null ? `@ ${String(f.fill_price)}` : ""}{" "}
                                      {f.fee != null ? `fee ${String(f.fee)}${f.fee_asset ? ` ${f.fee_asset}` : ""}` : ""}
                                      <br />
                                      <span style={{ opacity: 0.85 }}>{f.event_time}</span>
                                    </div>
                                  ))
                                )}
                              </div>
                            </div>
                          ) : null}
                          <p className="tv-drawer__section-head" style={{ marginTop: "0.75rem" }}>
                            {t("app.engineDrawer.snapshotSummaryHead")}
                          </p>
                          <div
                            style={{ maxHeight: "10rem", overflow: "auto", fontSize: "0.72rem" }}
                            className="mono muted"
                          >
                            {engineSnapshots.length === 0 ? (
                              <span className="err">{t("app.engineDrawer.snapshotEmpty")}</span>
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
                            {t("app.engineDrawer.confluenceSummaryHead")}
                          </p>
                          <div
                            style={{ maxHeight: "6rem", overflow: "auto", fontSize: "0.72rem" }}
                            className="mono muted"
                          >
                            {engineSnapshots.filter((s) => s.engine_kind === "confluence").length === 0 ? (
                              <span>{t("app.engineDrawer.confluenceEmpty")}</span>
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
                            <code>GET …/analysis/market-context/latest</code>:{" "}
                            <HelpCrossLink topicId="engine-market-context-latest" onOpen={jumpToHelp} label="SSS" />
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
                        . Uçlar ve worker kurulumu için{" "}
                        <HelpCrossLink topicId="market-context-overview" onOpen={jumpToHelp} label="Yardım" /> · özet
                        filtresi:{" "}
                        <HelpCrossLink topicId="market-context-summary" onOpen={jumpToHelp} label="Özet" />
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
                        <code>GET …/market-context/summary</code> süzümü:{" "}
                        <HelpCrossLink topicId="market-context-summary" onOpen={jumpToHelp} label="SSS" />
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
                              const yerel = trendAxisDisplayAsLongShort(
                                pickDashboardStr(v2?.local_trend, p?.yerel_trend),
                              );
                              const gbl = trendAxisDisplayAsLongShort(
                                pickDashboardStr(v2?.global_trend, p?.global_trend),
                              );
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
                        Özet ve ortam değişkenleri:{" "}
                        <HelpCrossLink topicId="nansen-token-screener" onOpen={jumpToHelp} label="Tam rehber" /> · resmi:{" "}
                        <a href="https://docs.nansen.ai/" target="_blank" rel="noreferrer">
                          docs.nansen.ai
                        </a>
                      </p>
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
                            Worker <code>setup_scan_engine</code>; tablo yapısı:{" "}
                            <HelpCrossLink topicId="nansen-token-screener" onOpen={jumpToHelp} label="SSS" />
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
                  {matchesSetting(
                    "kuyruk",
                    "queue",
                    "notify",
                    "outbox",
                    "bildirim",
                    "ai",
                    "karar",
                    "decision",
                    "onay",
                    "approval",
                    "ops",
                    "worker",
                  ) ? (
                    <AiDecisionsPanel accessToken={token} canAdmin={rbacIsAdmin} />
                  ) : null}
                </>
              ) : null}

              {drawerTab === "notify" ? (
                <>
                  {matchesSetting(
                    "notify",
                    "outbox",
                    "bildirim",
                    "test",
                    "kanal",
                    "channel",
                    "telegram",
                    "webhook",
                    "discord",
                    "email",
                    "smtp",
                    "kuyruk",
                  ) ? (
                    token ? (
                      <NotificationDrawerPanel accessToken={token} />
                    ) : (
                      <p className="muted card">{t("notifyTest.signInPrompt")}</p>
                    )
                  ) : null}
                </>
              ) : null}

              {drawerTab === "help" ? <HelpPanel query={drawerSearch} focusTopicId={helpFocusId} /> : null}

              {drawerTab === "setting" ? (
                <>
                  {token &&
                  rbacIsAdmin &&
                  matchesSetting(
                    "sistem",
                    "system",
                    "uygulama",
                    "application",
                    "parametre",
                    "parameter",
                    "sunucu",
                    "server",
                    "registry",
                    "telegram",
                    "notify",
                    "veritabanı",
                    "database",
                    "app_config",
                    "system_config",
                    "yapılandırma",
                    "configuration",
                    "credentials",
                    "bot",
                    "token",
                  ) ? (
                    <ServerRegistryPanel accessToken={token} />
                  ) : null}
                  {token && rbacIsAdmin ? (
                    <div className="card">
                      <p className="tv-drawer__section-head">Worker data sources</p>
                      <p className="muted" style={{ marginTop: 0, fontSize: "0.75rem" }}>
                        Bu anahtarlar <code>system_config</code> üzerinden worker döngülerini aç/kapa yapar. Kapalı iken sistem
                        çalışmaya devam eder, sadece ilgili kaynak verisi üretilmez.
                      </p>
                      <div className="tv-settings__fields">
                        <label style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}>
                          <input
                            type="checkbox"
                            checked={workerFlags.nansenEnabled}
                            onChange={(e) => {
                              const next = e.target.checked;
                              setWorkerFlags((s) => ({ ...s, nansenEnabled: next }));
                              if (!token) return;
                              void upsertAdminSystemConfig(token, {
                                module: "worker",
                                config_key: "nansen_enabled",
                                value: { enabled: next },
                                description: "Enable Nansen HTTP loops (credit burn control).",
                                is_secret: false,
                              }).catch(() => {});
                            }}
                          />
                          <span>Nansen</span>
                        </label>
                        <label style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}>
                          <input
                            type="checkbox"
                            checked={workerFlags.externalFetchEnabled}
                            onChange={(e) => {
                              const next = e.target.checked;
                              setWorkerFlags((s) => ({ ...s, externalFetchEnabled: next }));
                              if (!token) return;
                              void upsertAdminSystemConfig(token, {
                                module: "worker",
                                config_key: "external_fetch_enabled",
                                value: { enabled: next },
                                description: "Enable external_data_sources HTTP engines.",
                                is_secret: false,
                              }).catch(() => {});
                            }}
                          />
                          <span>External HTTP engines</span>
                        </label>
                      </div>
                    </div>
                  ) : null}
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
