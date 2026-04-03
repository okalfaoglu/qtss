export type ChartDefaults = {
  exchange: string;
  segment: string;
  symbol: string;
  interval: string;
  limit: string;
};

export function readChartDefaults(): ChartDefaults {
  return {
    exchange: import.meta.env.VITE_DEFAULT_EXCHANGE ?? "binance",
    segment: import.meta.env.VITE_DEFAULT_SEGMENT ?? "spot",
    symbol: (import.meta.env.VITE_DEFAULT_SYMBOL ?? "BTCUSDT").toUpperCase(),
    interval: import.meta.env.VITE_DEFAULT_INTERVAL ?? "15m",
    limit: String(import.meta.env.VITE_DEFAULT_BAR_LIMIT ?? "5000"),
  };
}

/** 0 = disable live candle polling. */
export function readLivePollMs(): number {
  const raw = import.meta.env.VITE_LIVE_POLL_MS;
  if (raw === "0" || raw === "false") return 0;
  const n = parseInt(String(raw ?? "5000"), 10);
  return Number.isFinite(n) && n >= 0 ? n : 5000;
}
