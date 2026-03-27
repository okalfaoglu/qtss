import { fetchBinanceKlinesAsChartRows } from "./binanceKlines";
import { fetchMarketBarsRecent, type MarketBarRow } from "./client";
import { CHART_INTERVALS } from "../lib/chartIntervals";
import { lastCandleLiveStatsFromRows, type LastCandleLiveStats } from "../lib/lastCandleLiveStats";
import type { ChartOhlcRow } from "../lib/marketBarsToCandles";

export type TimeframeLiveCell = {
  interval: string;
  stats: LastCandleLiveStats | null;
  error?: string;
};

function rowSlice(m: MarketBarRow): ChartOhlcRow {
  return {
    open_time: m.open_time,
    open: m.open,
    high: m.high,
    low: m.low,
    close: m.close,
  };
}

/**
 * Aynı sembol için tüm interval’lerin son 1–2 mumunu çekip anlık değişimleri üretir (TV’deki çoklu TF özetine benzer).
 * `Promise.allSettled`: tek interval hatası diğerlerini düşürmez.
 */
export async function fetchMultiTimeframeLiveCells(params: {
  symbol: string;
  intervals?: readonly string[];
  accessToken?: string | null;
  exchange?: string;
  segment?: string;
  /** Ana grafik ile aynı kaynak: `true` ise JWT olsa bile Binance REST (son mumlar). */
  ohlcFromBinanceRest?: boolean;
}): Promise<TimeframeLiveCell[]> {
  const sym = params.symbol.trim().toUpperCase();
  const intervals = params.intervals?.length ? [...params.intervals] : [...CHART_INTERVALS];
  if (!sym) {
    return intervals.map((interval) => ({ interval, stats: null, error: "Sembol boş" }));
  }

  const tok = params.accessToken?.trim();
  const ex = params.exchange?.trim() ?? "binance";
  const seg = params.segment?.trim() ?? "spot";
  const useBinanceRest =
    params.ohlcFromBinanceRest !== undefined ? params.ohlcFromBinanceRest : !tok;

  const tasks = intervals.map(
    (interval) =>
      (async (): Promise<TimeframeLiveCell> => {
        try {
          let rows: ChartOhlcRow[];
          if (useBinanceRest) {
            rows = await fetchBinanceKlinesAsChartRows({
              symbol: sym,
              interval,
              limit: 2,
              accessToken: tok ?? undefined,
              segment: seg,
            });
          } else if (tok) {
            const full = await fetchMarketBarsRecent(tok, {
              exchange: ex,
              segment: seg,
              symbol: sym,
              interval,
              limit: 2,
            });
            rows = full.map(rowSlice);
          } else {
            rows = await fetchBinanceKlinesAsChartRows({
              symbol: sym,
              interval,
              limit: 2,
              accessToken: undefined,
              segment: seg,
            });
          }
          return { interval, stats: lastCandleLiveStatsFromRows(rows) };
        } catch (e) {
          return { interval, stats: null, error: String(e) };
        }
      })(),
  );

  const settled = await Promise.allSettled(tasks);
  return settled.map((s, i) => {
    if (s.status === "fulfilled") return s.value;
    return { interval: intervals[i]!, stats: null, error: s.reason != null ? String(s.reason) : "Bilinmeyen hata" };
  });
}
