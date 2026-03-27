/** Grafik OHLC kaynağı: DB (`market_bars`) veya doğrudan Binance spot REST. */
export type ChartOhlcMode = "auto" | "exchange" | "database";

const LS_KEY = "qtss-chart-ohlc-mode";

export function readChartOhlcMode(): ChartOhlcMode {
  try {
    const ls = localStorage.getItem(LS_KEY);
    if (ls === "auto" || ls === "exchange" || ls === "database") return ls;
  } catch {
    /* private mode */
  }
  const env = import.meta.env.VITE_CHART_OHLC_MODE as string | undefined;
  if (env === "auto" || env === "exchange" || env === "database") return env;
  return "auto";
}

export function persistChartOhlcMode(m: ChartOhlcMode): void {
  try {
    localStorage.setItem(LS_KEY, m);
  } catch {
    /* ignore */
  }
}

/**
 * `true` → `fetchBinanceKlinesAsChartRows` (proxy / VITE_BINANCE_API_BASE).
 * `false` → girişli kullanıcı için `fetchMarketBarsRecent` (`market_bars`).
 *
 * - **auto:** Giriş yoksa her zaman REST. Giriş varsa yalnızca `binance` + `spot` iken REST (güncel mum);
 *   diğer borsa/segment için DB (mevcut davranış).
 * - **exchange:** Her zaman Binance spot REST ile ana grafik (sembol üst çubuktaki gibi).
 * - **database:** Tablo; JWT gerekir.
 */
export function chartUsesBinanceRestForOhlc(
  mode: ChartOhlcMode,
  token: string | null,
  exchange: string,
  segment: string,
): boolean {
  if (mode === "exchange") return true;
  if (mode === "database") return false;
  const binanceSpot = exchange.trim().toLowerCase() === "binance" && segment.trim().toLowerCase() === "spot";
  if (!token?.trim()) return true;
  return binanceSpot;
}
