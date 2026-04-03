/** Align toolbar segment with `market_bars.segment` normalization. */
export function normalizeMarketSegment(segment: string): string {
  const s = segment.trim().toLowerCase();
  if (s === "futures" || s === "usdt_futures" || s === "fapi") return "futures";
  return s || "spot";
}

/** Toolbar `<select>` value (`usdt_futures` / `fapi` → futures). */
export function chartToolbarSegmentSelectValue(segment: string): "spot" | "futures" {
  return normalizeMarketSegment(segment) === "futures" ? "futures" : "spot";
}
