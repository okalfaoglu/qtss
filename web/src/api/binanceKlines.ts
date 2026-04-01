import type { ChartOhlcRow } from "../lib/marketBarsToCandles";
import { chartOhlcRowsSortedChrono } from "../lib/chartRowsToOhlcBars";
import { fetchMarketBinanceKlinesForChart } from "./client";
import { describeBinanceKlinesHttpFailure } from "./binanceKlinesErrorFormat";
import {
  normalizeSymbolForBinanceKlinesApi,
  symbolLooksCompleteForBinanceKlines,
} from "./binanceKlinesSymbol";

const BINANCE_INTERVALS = new Set([
  "1m",
  "3m",
  "5m",
  "15m",
  "30m",
  "1h",
  "2h",
  "4h",
  "6h",
  "8h",
  "12h",
  "1d",
  "3d",
  "1w",
  "1M",
]);

/** Binance / fapi tek istekte en fazla 1000 mum. */
const BINANCE_KLINE_MAX_PER_REQUEST = 1000;

/** Tarayıcıdan doğrudan (Vite proxy yok); `vite build` / `preview` / statik sunucu için. */
const BINANCE_SPOT_PUBLIC_DEFAULT = "https://api.binance.com";

function maxPagesEnv(): number {
  const raw = import.meta.env.VITE_BINANCE_KLINE_MAX_PAGES;
  const n = parseInt(String(raw ?? "50"), 10);
  return Math.min(200, Math.max(1, Number.isFinite(n) ? n : 50));
}

function isBinanceFuturesSegment(segment: string | undefined): boolean {
  const s = (segment ?? "spot").trim().toLowerCase();
  return s === "futures" || s === "usdt_futures" || s === "fapi";
}

/** Spot’ta olmayan USDT-M sembolleri (-1121); bir kez FAPI ile yeniden dene. */
function isBinanceInvalidSymbolKlines(bodyOrMessage: string): boolean {
  const s = bodyOrMessage;
  return s.includes("-1121") || /"code"\s*:\s*-1121/.test(s);
}

function klinesUrl(
  symbol: string,
  interval: string,
  limit: number,
  segment: string | undefined,
  startTimeMs?: number,
  endTimeMs?: number,
): string {
  const futures = isBinanceFuturesSegment(segment);
  const spotBaseEnv = (import.meta.env.VITE_BINANCE_API_BASE as string | undefined)?.replace(/\/$/, "") ?? "";
  const fapiBase =
    (import.meta.env.VITE_BINANCE_FAPI_API_BASE as string | undefined)?.replace(/\/$/, "") ?? "https://fapi.binance.com";
  /** Yalnızca `npm run dev`: `/__binance` köprüleri. Build/preview’da yok — doğrudan HTTPS. */
  const useViteBinanceProxy = import.meta.env.DEV && !spotBaseEnv;
  const q = new URLSearchParams({
    symbol: symbol.toUpperCase(),
    interval,
    limit: String(limit),
  });
  if (startTimeMs != null && Number.isFinite(startTimeMs)) {
    q.set("startTime", String(Math.floor(startTimeMs)));
  }
  if (endTimeMs != null && Number.isFinite(endTimeMs)) {
    q.set("endTime", String(Math.floor(endTimeMs)));
  }
  if (futures) {
    const path = `/fapi/v1/klines?${q}`;
    if (spotBaseEnv) return `${fapiBase}${path}`;
    if (useViteBinanceProxy) return `/__binance_fapi${path}`;
    return `${fapiBase}${path}`;
  }
  const path = `/api/v3/klines?${q}`;
  if (spotBaseEnv) return `${spotBaseEnv}${path}`;
  if (useViteBinanceProxy) return `/__binance${path}`;
  return `${BINANCE_SPOT_PUBLIC_DEFAULT}${path}`;
}

function binanceKlinesViaQtssApi(): boolean {
  const v = import.meta.env.VITE_BINANCE_KLINES_VIA_API;
  return v === "1" || v === "true" || String(v).toLowerCase() === "yes";
}

/** UI: giriş + env ile mumlar QTSS API → sunucu `fapi`/`spot` çağırır (tarayıcıda fapi host görünmeyebilir). */
export function binanceKlinesUsesQtssApi(accessToken: string | null | undefined): boolean {
  return Boolean(accessToken?.trim()) && binanceKlinesViaQtssApi();
}

export function binanceKlinesDebugLoggingEnabled(): boolean {
  const v = import.meta.env.VITE_BINANCE_KLINES_DEBUG;
  return v === "1" || v === "true" || String(v).toLowerCase() === "yes";
}

function debugKlines(message: string, details: Record<string, string>): void {
  if (!binanceKlinesDebugLoggingEnabled()) return;
  console.debug(`[qtss-klines] ${message}`, details);
}

function rowsFromBinanceJson(raw: unknown): ChartOhlcRow[] {
  if (!Array.isArray(raw)) throw new Error("Binance yanıtı dizi değil");
  const out: ChartOhlcRow[] = [];
  for (const row of raw) {
    if (!Array.isArray(row) || row.length < 5) continue;
    const tOpen = row[0];
    if (typeof tOpen !== "number") continue;
    out.push({
      open_time: new Date(tOpen).toISOString(),
      open: String(row[1]),
      high: String(row[2]),
      low: String(row[3]),
      close: String(row[4]),
    });
  }
  return out;
}

async function fetchBinanceKlinesOneRequest(params: {
  symbol: string;
  interval: string;
  limit: number;
  accessToken?: string | null;
  segment?: string;
  startTimeMs?: number;
  endTimeMs?: number;
}): Promise<ChartOhlcRow[]> {
  const sym = params.symbol.trim().toUpperCase();
  const iv = params.interval.trim();
  const lim = Math.min(BINANCE_KLINE_MAX_PER_REQUEST, Math.max(1, params.limit));
  const tok = params.accessToken?.trim();
  const seg0 = params.segment?.trim() || "spot";

  const runViaApi = async (segment: string) =>
    fetchMarketBinanceKlinesForChart(tok!, {
      symbol: sym,
      interval: iv,
      limit: lim,
      segment,
      startTimeMs: params.startTimeMs,
      endTimeMs: params.endTimeMs,
    });

  if (binanceKlinesViaQtssApi() && tok) {
    debugKlines("QTSS API route (upstream spot/fapi on server)", {
      segment: seg0,
      symbol: sym,
      interval: iv,
      limit: String(lim),
    });
    try {
      return await runViaApi(seg0);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      if (!isBinanceFuturesSegment(seg0) && isBinanceInvalidSymbolKlines(msg)) {
        return runViaApi("futures");
      }
      throw e;
    }
  }

  const segmentErrLabel = isBinanceFuturesSegment(seg0) ? "USDT-M" : "spot";

  const fetchDirectOnce = async (segment: string | undefined) => {
    const url = klinesUrl(sym, iv, lim, segment, params.startTimeMs, params.endTimeMs);
    debugKlines("browser direct URL", {
      segment: segment?.trim() || "spot",
      url,
      symbol: sym,
      interval: iv,
    });
    const r = await fetch(url);
    const t = await r.text();
    if (!r.ok) {
      return { ok: false as const, status: r.status, body: t, requestUrl: url };
    }
    let raw: unknown;
    try {
      raw = JSON.parse(t) as unknown;
    } catch {
      throw new Error("Binance klines: geçersiz JSON yanıtı");
    }
    return { ok: true as const, rows: rowsFromBinanceJson(raw) };
  };

  const first = await fetchDirectOnce(seg0);
  if (first.ok) return first.rows;
  if (!isBinanceFuturesSegment(seg0) && isBinanceInvalidSymbolKlines(first.body)) {
    const second = await fetchDirectOnce("futures");
    if (second.ok) return second.rows;
    throw new Error(
      describeBinanceKlinesHttpFailure(second.status, second.body, {
        requestUrl: second.requestUrl,
        segment: "USDT-M",
      }),
    );
  }
  throw new Error(
    describeBinanceKlinesHttpFailure(first.status, first.body, {
      requestUrl: first.requestUrl,
      segment: segmentErrLabel,
    }),
  );
}

/** `[rangeStartMs, rangeEndMs]` aralığındaki tüm mumlar (sayfalı, kronolojik). */
async function fetchBinanceKlinesInOpenTimeRange(params: {
  symbol: string;
  interval: string;
  accessToken?: string | null;
  segment?: string;
  rangeStartMs: number;
  rangeEndMs: number;
}): Promise<ChartOhlcRow[]> {
  let start = Math.floor(params.rangeStartMs);
  const end = Math.floor(params.rangeEndMs);
  if (end < start) return [];

  const acc: ChartOhlcRow[] = [];
  const maxPg = maxPagesEnv();
  for (let p = 0; p < maxPg; p++) {
    const batch = await fetchBinanceKlinesOneRequest({
      symbol: params.symbol,
      interval: params.interval,
      accessToken: params.accessToken,
      segment: params.segment,
      limit: BINANCE_KLINE_MAX_PER_REQUEST,
      startTimeMs: start,
      endTimeMs: end,
    });
    const sorted = chartOhlcRowsSortedChrono(batch);
    if (!sorted.length) break;
    acc.push(...sorted);
    const lastT = new Date(sorted[sorted.length - 1]!.open_time).getTime();
    if (lastT >= end) break;
    const nextStart = lastT + 1;
    if (nextStart <= start) break;
    start = nextStart;
  }
  return chartOhlcRowsSortedChrono(acc);
}

/** En yeni mumdan geriye doğru sayfalayarak toplam `totalBars` mum (üst sınır: sayfa × 1000). */
async function fetchBinanceKlinesRecentTotal(params: {
  symbol: string;
  interval: string;
  accessToken?: string | null;
  segment?: string;
  totalBars: number;
  /** En yeni sayfanın üst `endTime` sınırı (ms); verilmezse Binance “şimdi”. */
  endTimeMs?: number;
}): Promise<ChartOhlcRow[]> {
  const cap = Math.max(1, Math.floor(params.totalBars));
  const batches: ChartOhlcRow[][] = [];
  let endTimeMs: number | undefined = params.endTimeMs;
  let remaining = cap;
  const maxPg = maxPagesEnv();

  for (let p = 0; p < maxPg && remaining > 0; p++) {
    const lim = Math.min(BINANCE_KLINE_MAX_PER_REQUEST, remaining);
    const batch = await fetchBinanceKlinesOneRequest({
      symbol: params.symbol,
      interval: params.interval,
      accessToken: params.accessToken,
      segment: params.segment,
      limit: lim,
      endTimeMs,
    });
    const sorted = chartOhlcRowsSortedChrono(batch);
    if (!sorted.length) break;
    batches.unshift(sorted);
    const oldestMs = new Date(sorted[0]!.open_time).getTime();
    endTimeMs = oldestMs - 1;
    remaining -= sorted.length;
    if (sorted.length < lim) break;
  }

  const flat = chartOhlcRowsSortedChrono(batches.flat());
  if (flat.length > cap) return flat.slice(-cap);
  return flat;
}

/**
 * Binance klines → grafik satırı.
 * - Varsayılan: tarayıcı → Vite `/__binance` (spot) ve `/__binance_fapi` (USDT-M; `.env` → `VITE_BINANCE_PROXY_TARGET` / `VITE_BINANCE_FAPI_PROXY_TARGET`) veya doğrudan `VITE_BINANCE_API_BASE` + `VITE_BINANCE_FAPI_API_BASE` (CORS; OAuth gerekmez).
 * - `VITE_BINANCE_KLINES_VIA_API=1` ve `accessToken` doluysa: `GET /api/v1/market/binance/klines`.
 *
 * `startTimeMs` + `endTimeMs`: aynı takvim penceresi — limit aşılsa bile sayfalanır (MTF hizası için).
 * Yalnız `limit`: 1000’den büyükse geriye doğru sayfalanır.
 */
export async function fetchBinanceKlinesAsChartRows(params: {
  symbol: string;
  interval: string;
  limit?: number;
  accessToken?: string | null;
  segment?: string;
  startTimeMs?: number;
  endTimeMs?: number;
}): Promise<ChartOhlcRow[]> {
  const rawSym = params.symbol.trim();
  if (!rawSym) throw new Error("symbol boş");
  const sym = normalizeSymbolForBinanceKlinesApi(rawSym);
  if (!sym) throw new Error("symbol boş");
  if (!symbolLooksCompleteForBinanceKlines(sym, params.segment)) {
    return [];
  }
  const iv = params.interval.trim();
  if (!BINANCE_INTERVALS.has(iv)) throw new Error(`desteklenmeyen interval: ${iv}`);

  const hasStart = params.startTimeMs != null && Number.isFinite(params.startTimeMs);
  const hasEnd = params.endTimeMs != null && Number.isFinite(params.endTimeMs);

  if (hasStart && hasEnd) {
    return fetchBinanceKlinesInOpenTimeRange({
      symbol: sym,
      interval: iv,
      accessToken: params.accessToken,
      segment: params.segment,
      rangeStartMs: params.startTimeMs!,
      rangeEndMs: params.endTimeMs!,
    });
  }

  const limRaw = params.limit ?? 500;
  const want = Math.max(1, Math.floor(limRaw));

  if (want <= BINANCE_KLINE_MAX_PER_REQUEST) {
    return fetchBinanceKlinesOneRequest({
      symbol: sym,
      interval: iv,
      accessToken: params.accessToken,
      segment: params.segment,
      limit: want,
      startTimeMs: hasStart ? params.startTimeMs : undefined,
      endTimeMs: hasEnd ? params.endTimeMs : undefined,
    });
  }

  return fetchBinanceKlinesRecentTotal({
    symbol: sym,
    interval: iv,
    accessToken: params.accessToken,
    segment: params.segment,
    totalBars: want,
    endTimeMs: hasEnd ? params.endTimeMs : undefined,
  });
}
