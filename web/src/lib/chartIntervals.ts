/** Grafik ve çoklu zaman dilimi canlı şeridi için ortak liste (Binance spot ile uyumlu). */
export const CHART_INTERVALS = [
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
] as const;

export type ChartInterval = (typeof CHART_INTERVALS)[number];
