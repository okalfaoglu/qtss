import type { OhlcV2, ZigzagParams, ZigzagPivot } from "./types";

function normalizeParams(p: ZigzagParams): ZigzagParams {
  return {
    depth: Math.max(2, Math.floor(p.depth || 0)),
    deviationPct: Math.max(0, Number.isFinite(p.deviationPct) ? p.deviationPct : 0),
    backstep: Math.max(1, Math.floor(p.backstep || 0)),
  };
}

function pctMove(from: number, to: number): number {
  if (!Number.isFinite(from) || !Number.isFinite(to)) return 0;
  const d = Math.abs(from) > 1e-12 ? Math.abs(from) : 1;
  return Math.abs((to - from) / d);
}

/** 1. faz: depth penceresindeki yerel tepe/dip adayları (TradingView ZigZag ile uyumlu). */
function buildRawFractalPivots(rows: OhlcV2[], input: ZigzagParams): ZigzagPivot[] {
  const p = normalizeParams(input);
  const n = rows.length;
  if (n < p.depth * 2 + 1) return [];

  const raw: ZigzagPivot[] = [];
  for (let i = p.depth; i < n - p.depth; i++) {
    const cur = rows[i];
    if (!cur) continue;

    let isHigh = true;
    let isLow = true;

    for (let j = i - p.depth; j <= i + p.depth; j++) {
      if (j === i) continue;
      if (rows[j].h > cur.h) isHigh = false;
      if (rows[j].l < cur.l) isLow = false;
      if (!isHigh && !isLow) break;
    }

    if (isHigh && !isLow) {
      raw.push({ index: i, time: cur.t, price: cur.h, kind: "high" });
    } else if (isLow && !isHigh) {
      raw.push({ index: i, time: cur.t, price: cur.l, kind: "low" });
    }
  }
  return raw;
}

/** Sapma filtresi: `out` üzerine sırayla uygulanır (motor ile aynı kurallar). */
function mergeDeviationChain(out: ZigzagPivot[], incoming: ZigzagPivot[], dev: number): void {
  for (const pivot of incoming) {
    const last = out[out.length - 1];
    if (!last) {
      out.push(pivot);
      continue;
    }
    if (pivot.index === last.index) continue;

    const move = pctMove(last.price, pivot.price);

    if (pivot.kind === last.kind) {
      const better = pivot.kind === "high" ? pivot.price > last.price : pivot.price < last.price;
      if (better) {
        out[out.length - 1] = pivot;
      }
    } else {
      if (move >= dev) {
        out.push(pivot);
      }
    }
  }
}

/**
 * Onaylanmış pivotları depth + deviation kurallarına göre oluşturur.
 * Asenkron çalışan motorlar için tamamen deterministik ve side-effect free olarak tasarlanmıştır.
 */
export function buildZigzagPivotsV2(rows: OhlcV2[], input: ZigzagParams): ZigzagPivot[] {
  const p = normalizeParams(input);
  const n = rows.length;
  if (n < p.depth * 2 + 1) return [];

  const dev = p.deviationPct / 100;
  const raw = buildRawFractalPivots(rows, p);
  const out: ZigzagPivot[] = [];
  mergeDeviationChain(out, raw, dev);
  return out;
}

/**
 * Son muma kadar kısa bir segment: çizgi grafiğin sağ ucuna uzanır (fitil ucu).
 */
function appendSegmentToLastBar(out: ZigzagPivot[], rows: OhlcV2[]): void {
  const last = out[out.length - 1];
  const n = rows.length;
  if (!last || last.index >= n - 1) return;
  const b = rows[n - 1];
  if (last.kind === "high") {
    out.push({ index: n - 1, time: b.t, price: b.l, kind: "low" });
  } else {
    out.push({ index: n - 1, time: b.t, price: b.h, kind: "high" });
  }
}

/**
 * Grafik tarafı: motor pivotları aynı kalır. Çizgi, son onaylı pivottan sonra kalan bölgede
 * ham fraktallar + aynı sapma birleştirmesiyle devam eder — tek parça “global min/max” çizgisi
 * aradaki tepkiyi atlayıp mumların içinden kesmez.
 */
export function extendZigzagPivotsForChartLine(
  rows: OhlcV2[],
  pivots: ZigzagPivot[],
  input: ZigzagParams,
): ZigzagPivot[] {
  if (pivots.length < 2 || rows.length < 2) return pivots;
  const p = normalizeParams(input);
  const n = rows.length;
  const last = pivots[pivots.length - 1];
  if (last.index >= n - 1) return pivots;

  const dev = p.deviationPct / 100;
  const raw = buildRawFractalPivots(rows, p);
  const rawAfter = raw.filter((x) => x.index > last.index);

  const tail: ZigzagPivot[] = [{ ...last }];
  mergeDeviationChain(tail, rawAfter, dev);

  const merged = [...pivots.slice(0, -1), ...tail];
  const fin = merged[merged.length - 1];
  if (fin.index < n - 1) {
    appendSegmentToLastBar(merged, rows);
  }

  return merged;
}
