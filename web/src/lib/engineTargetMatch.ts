/** Aligns `engine_symbols` / snapshot segment with chart toolbar (`usdt_futures` / `fapi` → futures). */
export function normalizeEngineMarketSegment(segment: string): string {
  const s = segment.trim().toLowerCase();
  if (s === "futures" || s === "usdt_futures" || s === "fapi") return "futures";
  return s || "spot";
}

export type EngineTargetLookup = {
  exchange: string;
  segment: string;
  symbol: string;
  interval: string;
};

export function engineRowMatchesTarget(
  row: { exchange: string; segment: string; symbol: string; interval: string },
  target: EngineTargetLookup,
): boolean {
  const exOk = row.exchange.trim().toLowerCase() === target.exchange.trim().toLowerCase();
  const segOk = normalizeEngineMarketSegment(row.segment) === normalizeEngineMarketSegment(target.segment);
  const symOk = row.symbol.trim().toUpperCase() === target.symbol.trim().toUpperCase();
  const ivOk = row.interval.trim() === target.interval.trim();
  return exOk && segOk && symOk && ivOk;
}
