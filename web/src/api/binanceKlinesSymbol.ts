/**
 * Normalize user-facing symbols (e.g. TradingView `TRADOORUSDT.P`, `BINANCE:BTCUSDT`) for Binance REST klines.
 * Venue tickers do not use dot suffixes or exchange prefixes.
 */
export function normalizeSymbolForBinanceKlinesApi(symbol: string): string {
  let s = symbol.trim().toUpperCase();
  const colon = s.indexOf(":");
  if (colon >= 0) {
    s = s.slice(colon + 1).trim();
  }
  if (s.endsWith(".P")) {
    s = s.slice(0, -2);
  } else if (s.endsWith(".PERP")) {
    s = s.slice(0, -5);
  }
  return s.trim();
}

const USDM_FUTURES_KLINES_SEGMENTS = new Set(["futures", "usdt_futures", "fapi"]);

function isUsdmFuturesKlinesSegment(segment: string | undefined): boolean {
  return USDM_FUTURES_KLINES_SEGMENTS.has((segment ?? "spot").trim().toLowerCase());
}

const SPOT_KLINES_QUOTE_SUFFIXES = [
  "USDT",
  "USDC",
  "FDUSD",
  "BUSD",
  "TUSD",
  "BTC",
  "ETH",
  "BNB",
] as const;

/**
 * False for typeahead prefixes (e.g. "T", "TRADO") so callers skip Binance klines REST and avoid HTTP 400.
 * Expects the same shape as after {@link normalizeSymbolForBinanceKlinesApi}.
 */
export function symbolLooksCompleteForBinanceKlines(
  normalizedSymbol: string,
  segment: string | undefined,
): boolean {
  const s = normalizedSymbol.trim().toUpperCase();
  if (s.length < 6) return false;

  if (isUsdmFuturesKlinesSegment(segment)) {
    return s.endsWith("USDT") || s.endsWith("USDC") || s.endsWith("BUSD");
  }

  return SPOT_KLINES_QUOTE_SUFFIXES.some((q) => s.endsWith(q));
}
