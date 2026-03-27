import type { UTCTimestamp } from "lightweight-charts";
import type { ChartOhlcRow } from "./marketBarsToCandles";

export type PivotKind = "high" | "low";

export type SwingPivot = {
  barIndex: number;
  kind: PivotKind;
  price: number;
  time: UTCTimestamp;
};

function parseOHLC(row: ChartOhlcRow): { h: number; l: number } | null {
  const h = parseFloat(String(row.high));
  const l = parseFloat(String(row.low));
  if (!Number.isFinite(h) || !Number.isFinite(l)) return null;
  return { h, l };
}

function rowTimeSec(row: ChartOhlcRow): UTCTimestamp | null {
  const t = Math.floor(new Date(row.open_time).getTime() / 1000);
  return Number.isFinite(t) ? (t as UTCTimestamp) : null;
}

/**
 * Her iki yanında `depth` mum olan tepe/dip (geçmişe dönük analiz; “onaylı” pivot).
 * Elliott V2 dışında panelde swing sayımı için kullanılır.
 */
export function buildSwingPivots(bars: ChartOhlcRow[], depth: number): SwingPivot[] {
  const d = Math.max(1, Math.floor(depth));
  const n = bars.length;
  if (n < d * 2 + 1) return [];

  const highs: number[] = [];
  const lows: number[] = [];
  for (let i = 0; i < n; i++) {
    const o = parseOHLC(bars[i]);
    if (!o) return [];
    highs.push(o.h);
    lows.push(o.l);
  }

  type Ev = { idx: number; kind: PivotKind; price: number };
  const raw: Ev[] = [];

  for (let i = d; i < n - d; i++) {
    let isH = true;
    let isL = true;
    for (let j = i - d; j <= i + d; j++) {
      if (j === i) continue;
      if (highs[j] >= highs[i]) isH = false;
      if (lows[j] <= lows[i]) isL = false;
    }
    if (isH && isL) {
      const mid = (highs[i] + lows[i]) / 2;
      if (highs[i] - mid >= mid - lows[i]) raw.push({ idx: i, kind: "high", price: highs[i] });
      else raw.push({ idx: i, kind: "low", price: lows[i] });
    } else if (isH) raw.push({ idx: i, kind: "high", price: highs[i] });
    else if (isL) raw.push({ idx: i, kind: "low", price: lows[i] });
  }

  raw.sort((a, b) => a.idx - b.idx);
  const merged: Ev[] = [];
  for (const e of raw) {
    const last = merged[merged.length - 1];
    if (!last) {
      merged.push(e);
      continue;
    }
    if (last.kind !== e.kind) {
      merged.push(e);
      continue;
    }
    if (e.kind === "high") {
      if (e.price >= last.price) merged[merged.length - 1] = e;
    } else if (e.price <= last.price) {
      merged[merged.length - 1] = e;
    }
  }

  const out: SwingPivot[] = [];
  for (const e of merged) {
    const row = bars[e.idx];
    const t = rowTimeSec(row);
    if (t == null) continue;
    out.push({ barIndex: e.idx, kind: e.kind, price: e.price, time: t });
  }
  return out;
}
