import type { ChartOhlcRow } from "../lib/marketBarsToCandles";
import { chartOhlcRowsSortedChrono } from "../lib/chartRowsToOhlcBars";
import { fetchMarketBinanceKlinesForChart } from "./client";

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

function maxPagesEnv(): number {
  const raw = import.meta.env.VITE_BINANCE_KLINE_MAX_PAGES;
  const n = parseInt(String(raw ?? "50"), 10);
  return Math.min(200, Math.max(1, Number.isFinite(n) ? n : 50));
}

function isBinanceFuturesSegment(segment: string | undefined): boolean {
  const s = (segment ?? "spot").trim().toLowerCase();
  return s === "futures" || s === "usdt_futures" || s === "fapi";
}

function formatBinanceKlinesError(status: number, body: string): string {
  const slice = body.slice(0, 280);
  try {
    const j = JSON.parse(body) as { code?: number; msg?: string };
    if (j.code === -1121) {
      return `Binance klines ${status}: Geçersiz sembol (-1121). Çift spot’ta yoksa segment’i USDT vadeli (futures/usdt_futures) yapın; yine olmazsa sembolü Binance’te doğrulayın. Ham: ${slice}`;
    }
  } catch {
    /* ignore */
  }
  return `Binance klines ${status}: ${slice}`;
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
  const spotBase = (import.meta.env.VITE_BINANCE_API_BASE as string | undefined)?.replace(/\/$/, "") ?? "";
  const fapiBase =
    (import.meta.env.VITE_BINANCE_FAPI_API_BASE as string | undefined)?.replace(/\/$/, "") ?? "https://fapi.binance.com";
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
    if (spotBase) return `${fapiBase}${path}`;
    return `/__binance_fapi${path}`;
  }
  const path = `/api/v3/klines?${q}`;
  if (spotBase) return `${spotBase}${path}`;
  return `/__binance${path}`;
}

function binanceKlinesViaQtssApi(): boolean {
  const v = import.meta.env.VITE_BINANCE_KLINES_VIA_API;
  return v === "1" || v === "true" || String(v).toLowerCase() === "yes";
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
  if (binanceKlinesViaQtssApi() && tok) {
    return fetchMarketBinanceKlinesForChart(tok, {
      symbol: sym,
      interval: iv,
      limit: lim,
      segment: params.segment?.trim() || "spot",
      startTimeMs: params.startTimeMs,
      endTimeMs: params.endTimeMs,
    });
  }

  const url = klinesUrl(sym, iv, lim, params.segment, params.startTimeMs, params.endTimeMs);
  const r = await fetch(url);
  if (!r.ok) {
    const t = await r.text();
    throw new Error(formatBinanceKlinesError(r.status, t));
  }
  const raw = (await r.json()) as unknown;
  return rowsFromBinanceJson(raw);
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
  const sym = params.symbol.trim().toUpperCase();
  const iv = params.interval.trim();
  if (!sym) throw new Error("symbol boş");
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
