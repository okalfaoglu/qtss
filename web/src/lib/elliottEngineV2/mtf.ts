import type { OhlcV2, Timeframe } from "./types";

function bucketize(rows: OhlcV2[], bucketSec: number): OhlcV2[] {
  if (!rows.length || bucketSec <= 0) return [];
  const sorted = [...rows].sort((a, b) => a.t - b.t);
  const out: OhlcV2[] = [];

  let curKey = -1;
  let cur: OhlcV2 | null = null;
  for (const r of sorted) {
    const key = Math.floor(r.t / bucketSec);
    if (key !== curKey) {
      if (cur) out.push(cur);
      curKey = key;
      cur = { ...r, t: key * bucketSec };
      continue;
    }
    if (!cur) continue;
    if (r.h > cur.h) cur.h = r.h;
    if (r.l < cur.l) cur.l = r.l;
    cur.c = r.c;
  }
  if (cur) out.push(cur);
  return out;
}

function tfSec(tf: Timeframe): number {
  if (tf === "15m") return 15 * 60;
  if (tf === "1h") return 60 * 60;
  return 4 * 60 * 60;
}

/**
 * Build V2 frame map from current chart anchor.
 * - 15m anchor -> 15m + 1h + 4h
 * - 1h anchor -> 1h + 4h
 * - 4h anchor -> 4h
 */
export function buildMtfFramesV2(anchor: OhlcV2[], anchorTf: Timeframe): Partial<Record<Timeframe, OhlcV2[]>> {
  const out: Partial<Record<Timeframe, OhlcV2[]>> = {};
  if (!anchor.length) return out;

  const uniq = [...anchor].sort((a, b) => a.t - b.t);
  out[anchorTf] = uniq;

  if (anchorTf === "15m") {
    out["1h"] = bucketize(uniq, tfSec("1h"));
    out["4h"] = bucketize(uniq, tfSec("4h"));
  } else if (anchorTf === "1h") {
    out["4h"] = bucketize(uniq, tfSec("4h"));
  }

  return out;
}

