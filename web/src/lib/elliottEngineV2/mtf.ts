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

export function tfSec(tf: Timeframe): number {
  if (tf === "15m") return 15 * 60;
  if (tf === "1h") return 60 * 60;
  if (tf === "4h") return 4 * 60 * 60;
  if (tf === "1d") return 24 * 60 * 60;
  if (tf === "1w") return 7 * 24 * 60 * 60;
  return 4 * 60 * 60;
}

/** Map chart interval string to Elliott anchor timeframe (fallback `15m`). */
export function chartIntervalToAnchorTimeframe(iv: string): Timeframe {
  const t = iv.trim().toLowerCase();
  if (t === "1w" || t === "1wk") return "1w";
  if (t === "1d" || t === "d") return "1d";
  if (t === "4h") return "4h";
  if (t === "1h") return "1h";
  return "15m";
}

/**
 * Build V2 frame map from current chart anchor (aligned buckets upward).
 * Finer anchors include all coarser TFs present in the series (e.g. 15m → … → 1w).
 */
export function buildMtfFramesV2(anchor: OhlcV2[], anchorTf: Timeframe): Partial<Record<Timeframe, OhlcV2[]>> {
  const out: Partial<Record<Timeframe, OhlcV2[]>> = {};
  if (!anchor.length) return out;

  const uniq = [...anchor].sort((a, b) => a.t - b.t);
  out[anchorTf] = uniq;

  if (anchorTf === "15m") {
    out["1h"] = bucketize(uniq, tfSec("1h"));
    out["4h"] = bucketize(uniq, tfSec("4h"));
    out["1d"] = bucketize(uniq, tfSec("1d"));
    out["1w"] = bucketize(uniq, tfSec("1w"));
  } else if (anchorTf === "1h") {
    out["4h"] = bucketize(uniq, tfSec("4h"));
    out["1d"] = bucketize(uniq, tfSec("1d"));
    out["1w"] = bucketize(uniq, tfSec("1w"));
  } else if (anchorTf === "4h") {
    out["1d"] = bucketize(uniq, tfSec("1d"));
    out["1w"] = bucketize(uniq, tfSec("1w"));
  } else if (anchorTf === "1d") {
    out["1w"] = bucketize(uniq, tfSec("1w"));
  }

  return out;
}

