import type { SeriesMarker, UTCTimestamp } from "lightweight-charts";
import { chartOhlcRowsSortedChrono } from "./chartRowsToOhlcBars";
import type { ChartOhlcRow } from "./marketBarsToCandles";
import type { PatternLayerOverlay } from "./patternDrawingBatchOverlay";

export type TradingRangeDbPayload = {
  range_high?: number;
  range_low?: number;
  mid?: number;
  valid?: boolean;
  is_range_regime?: boolean;
  /** Rejim sonrası etkin süpürme (API eski sürüm). */
  long_sweep_signal?: boolean;
  short_sweep_signal?: boolean;
  /** Ham fiyat süpürmesi — grafik checkbox için (rejim kapalı olsa da). */
  long_sweep_latent?: boolean;
  short_sweep_latent?: boolean;
  atr?: number;
  atr_sma?: number;
  chart_window_start_open_time?: string;
  chart_window_end_open_time?: string;
  last_bar_open_time?: string;
  reason?: string;
  support_touches?: number;
  resistance_touches?: number;
  close_breakout?: boolean;
  range_width?: number;
  range_width_atr?: number;
  range_too_narrow?: boolean;
  range_too_wide?: boolean;
  wick_rejection_long?: boolean;
  wick_rejection_short?: boolean;
  fake_breakout_long?: boolean;
  fake_breakout_short?: boolean;
  setup_score_long?: number;
  setup_score_short?: number;
  setup_score_best?: number;
  guardrails_pass?: boolean;
  setup_side?: string;
  score_touch_long?: number;
  score_touch_short?: number;
  score_rejection_long?: number;
  score_rejection_short?: number;
  score_oscillator_long?: number;
  score_oscillator_short?: number;
  score_volume_long?: number;
  score_volume_short?: number;
  score_breakout_long?: number;
  score_breakout_short?: number;
  volume_unavailable?: boolean;
  /** `upper` | `mid` | `lower` — range edge filter (A+ zone). */
  range_zone?: string;
};

function rowTime(row: ChartOhlcRow): UTCTimestamp | null {
  const t = Math.floor(new Date(row.open_time).getTime() / 1000);
  return Number.isFinite(t) ? (t as UTCTimestamp) : null;
}

function truthyBool(v: unknown): boolean {
  if (v === true) return true;
  if (v === false || v == null) return false;
  if (typeof v === "number" && Number.isFinite(v)) return v !== 0;
  if (typeof v === "string") {
    const s = v.trim().toLowerCase();
    return s === "true" || s === "1" || s === "yes";
  }
  return false;
}

/** Grafik: latent (ham fiyat süpürmesi) veya etkin sinyal — işaret göstermek için birleşik. */
function chartSweepFlags(p: TradingRangeDbPayload): { long: boolean; short: boolean } {
  const raw = p as Record<string, unknown>;
  const sigLong = truthyBool(p.long_sweep_signal);
  const sigShort = truthyBool(p.short_sweep_signal);
  if ("long_sweep_latent" in raw || "short_sweep_latent" in raw) {
    return {
      long: truthyBool(p.long_sweep_latent) || sigLong,
      short: truthyBool(p.short_sweep_latent) || sigShort,
    };
  }
  return {
    long: sigLong,
    short: sigShort,
  };
}

/** `analysis_snapshots.payload` (trading_range) → yatay üç çizgi (üst / alt / orta). */
export function patternLayerFromDbTradingRange(
  bars: ChartOhlcRow[],
  payload: unknown,
): PatternLayerOverlay | null {
  if (!payload || typeof payload !== "object") return null;
  const p = payload as TradingRangeDbPayload;
  const rh = p.range_high;
  const rl = p.range_low;
  const mid = p.mid;
  if (
    typeof rh !== "number" ||
    typeof rl !== "number" ||
    typeof mid !== "number" ||
    !Number.isFinite(rh) ||
    !Number.isFinite(rl) ||
    !Number.isFinite(mid)
  ) {
    return null;
  }

  const ch = chartOhlcRowsSortedChrono(bars);
  if (!ch.length) return null;
  const t0 = rowTime(ch[0]!);
  const t1 = rowTime(ch[ch.length - 1]!);
  if (t0 == null || t1 == null) return null;

  return {
    upper: [
      { time: t0, value: rh },
      { time: t1, value: rh },
    ],
    lower: [
      { time: t0, value: rl },
      { time: t1, value: rl },
    ],
    zigzag: [
      { time: t0, value: mid },
      { time: t1, value: mid },
    ],
    zigzagKind: "trading_range_mid",
    zigzagLineColor: "#ffb300",
    zigzagLineStyle: "dotted",
    zigzagLineWidth: 2,
  };
}

/** Son mumda likidite süpürme işareti (`trading_range` veya `signal_dashboard` snapshot yükü). */
export function sweepMarkersFromDbTradingRange(
  bars: ChartOhlcRow[],
  payload: unknown,
): SeriesMarker<UTCTimestamp>[] {
  if (!payload || typeof payload !== "object") return [];
  const p = payload as TradingRangeDbPayload;
  if (p.reason === "insufficient_bars") return [];

  const ch = chartOhlcRowsSortedChrono(bars);
  if (!ch.length) return [];

  const barTimes = new Set<number>();
  for (const r of ch) {
    const s = Math.floor(new Date(r.open_time).getTime() / 1000);
    if (Number.isFinite(s)) barTimes.add(s);
  }

  let t: UTCTimestamp | null = null;
  if (p.last_bar_open_time) {
    const sec = Math.floor(new Date(p.last_bar_open_time).getTime() / 1000);
    if (Number.isFinite(sec) && barTimes.has(sec)) {
      t = sec as UTCTimestamp;
    }
  }
  if (t == null) {
    const last = ch[ch.length - 1]!;
    t = rowTime(last);
  }
  if (t == null) return [];

  const { long: longSweep, short: shortSweep } = chartSweepFlags(p);
  const out: SeriesMarker<UTCTimestamp>[] = [];
  if (longSweep) {
    out.push({
      time: t,
      position: "belowBar",
      shape: "arrowUp",
      color: "#089981",
      text: "L sweep",
    });
  }
  if (shortSweep) {
    out.push({
      time: t,
      position: "aboveBar",
      shape: "arrowDown",
      color: "#f23645",
      text: "S sweep",
    });
  }
  return out;
}
