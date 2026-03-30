/**
 * Derive fill / fee / quote notionals from stored Binance `venue_response` JSON.
 */

export type BinanceFillMetrics = {
  executedQty: number;
  quoteQty: number;
  fee: number;
  feeAsset: string | null;
  status: string | null;
  ok: boolean;
};

export function binanceVenueFillMetrics(venueResponse: unknown): BinanceFillMetrics | null {
  if (venueResponse === null || typeof venueResponse !== "object") return null;
  const v = venueResponse as Record<string, unknown>;
  const status = typeof v.status === "string" ? v.status : null;
  const ok = status === "FILLED" || status === "PARTIALLY_FILLED";
  const exRaw = v.executedQty;
  const executedQty =
    typeof exRaw === "string" ? parseFloat(exRaw) : typeof exRaw === "number" ? exRaw : 0;
  if (!Number.isFinite(executedQty) || executedQty <= 0) return null;
  if (!ok) return null;

  const cqRaw = v.cummulativeQuoteQty ?? v.cumQuote;
  let quoteQty =
    typeof cqRaw === "string" ? parseFloat(cqRaw) : typeof cqRaw === "number" ? cqRaw : 0;
  if (!Number.isFinite(quoteQty)) quoteQty = 0;

  let fee = 0;
  let feeAsset: string | null = null;
  const fills = v.fills;
  if (Array.isArray(fills)) {
    for (const f of fills) {
      if (f === null || typeof f !== "object") continue;
      const o = f as Record<string, unknown>;
      const c = o.commission;
      if (typeof c === "string") {
        const x = parseFloat(c);
        if (Number.isFinite(x)) fee += x;
      }
      if (feeAsset == null && typeof o.commissionAsset === "string") feeAsset = o.commissionAsset;
    }
  }

  return {
    executedQty,
    quoteQty,
    fee,
    feeAsset,
    status,
    ok: true,
  };
}
