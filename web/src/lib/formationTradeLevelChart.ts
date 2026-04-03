import type { SeriesMarker, UTCTimestamp } from "lightweight-charts";
import type { ChannelSixResponse, FormationTradeLevelsJson, PatternMatchPayloadJson } from "../api/client";
import type { ChartOhlcRow } from "./marketBarsToCandles";
import { outcomePivotBarRange } from "./channelSixLiveSignal";

export type TradeLevelLineSpec = {
  segment: [{ time: UTCTimestamp; value: number }, { time: UTCTimestamp; value: number }];
  marker: SeriesMarker<UTCTimestamp>;
};

function matchPayloads(res: ChannelSixResponse): PatternMatchPayloadJson[] {
  if (res.pattern_matches?.length) return res.pattern_matches;
  if (res.outcome) {
    return [
      {
        outcome: res.outcome,
        pattern_name: res.pattern_name,
        pattern_drawing_batch: res.pattern_drawing_batch,
        formation_trade_levels: res.formation_trade_levels,
      },
    ];
  }
  return [];
}

/** Per-row trade levels; row 0 may fall back to root `formation_trade_levels` on legacy payloads. */
export function formationLevelsForMatchRow(
  res: ChannelSixResponse,
  m: PatternMatchPayloadJson,
  rowIndex: number,
): FormationTradeLevelsJson | undefined {
  if (m.formation_trade_levels) return m.formation_trade_levels;
  if (rowIndex === 0 && res.formation_trade_levels) return res.formation_trade_levels;
  return undefined;
}

function rowTimeSec(row: ChartOhlcRow): number | null {
  const t = Math.floor(new Date(row.open_time).getTime() / 1000);
  return Number.isFinite(t) ? t : null;
}

function avgBarSeconds(scanBars: ChartOhlcRow[]): number {
  if (scanBars.length < 2) return 3600;
  const t0 = rowTimeSec(scanBars[0]!) ?? 0;
  const t1 = rowTimeSec(scanBars[scanBars.length - 1]!) ?? 0;
  const span = Math.max(t1 - t0, 60);
  return Math.max(60, span / Math.max(1, scanBars.length - 1));
}

/** UNIX seconds for a scan-window bar index; extrapolate past the slice. */
function timeSecondsAtScanBarIndex(scanBars: ChartOhlcRow[], barIndex: number): number | null {
  if (!scanBars.length) return null;
  const lastI = scanBars.length - 1;
  if (barIndex >= 0 && barIndex < scanBars.length) {
    return rowTimeSec(scanBars[barIndex]!) ?? null;
  }
  const lastT = rowTimeSec(scanBars[lastI]!) ?? null;
  if (lastT == null) return null;
  const dt = avgBarSeconds(scanBars);
  const extra = barIndex - lastI;
  return lastT + extra * dt;
}

function timeSpanRightOfFormation(
  scanBars: ChartOhlcRow[],
  maxPivotBar: number,
  offsetBars: number,
  halfWidthBars: number,
): { tL: UTCTimestamp; tR: UTCTimestamp } | null {
  const anchor = maxPivotBar + offsetBars;
  const tLsec = timeSecondsAtScanBarIndex(scanBars, anchor - halfWidthBars);
  const tRsec = timeSecondsAtScanBarIndex(scanBars, anchor + halfWidthBars);
  if (tLsec == null || tRsec == null) return null;
  if (tRsec <= tLsec) return null;
  return { tL: tLsec as UTCTimestamp, tR: tRsec as UTCTimestamp };
}

function fmtPrice(n: number): string {
  const a = Math.abs(n);
  if (a >= 10_000) return n.toFixed(2);
  if (a >= 1) return n.toFixed(4);
  return n.toFixed(6);
}

const ROW_PALETTE = ["#26a69a", "#7e57c2", "#ff9800", "#42a5f5", "#ec407a", "#66bb6a"];

/**
 * Short horizontal line segments to the right of each formation + circle markers with Enter / SL / TP labels.
 */
export function buildFormationTradeLevelSpecs(
  res: ChannelSixResponse | null,
  scanBarsChrono: ChartOhlcRow[],
  options?: { offsetBars?: number; halfWidthBars?: number; onlyMatchRowIndex?: number },
): TradeLevelLineSpec[] {
  if (!res?.matched || !scanBarsChrono.length) return [];
  const offsetBars = options?.offsetBars ?? 4;
  const halfWidthBars = Math.max(1, options?.halfWidthBars ?? 2);
  const onlyRow = options?.onlyMatchRowIndex;

  const rows = matchPayloads(res);
  const specs: TradeLevelLineSpec[] = [];

  for (let i = 0; i < rows.length; i++) {
    if (onlyRow !== undefined && i !== onlyRow) continue;
    const m = rows[i]!;
    const levels = formationLevelsForMatchRow(res, m, i);
    if (!levels) continue;
    const br = outcomePivotBarRange(m.outcome);
    const maxB = br?.max ?? 0;
    const span = timeSpanRightOfFormation(scanBarsChrono, maxB, offsetBars, halfWidthBars);
    if (!span) continue;
    const { tL, tR } = span;
    const rowHue = ROW_PALETTE[i % ROW_PALETTE.length]!;

    const push = (price: number, text: string, color: string) => {
      specs.push({
        segment: [
          { time: tL, value: price },
          { time: tR, value: price },
        ],
        marker: {
          time: tR,
          position: "inBar",
          color,
          shape: "circle",
          text,
        },
      });
    };

    push(levels.entry, `Enter: ${fmtPrice(levels.entry)}`, rowHue);
    push(levels.stop_loss, `SL: ${fmtPrice(levels.stop_loss)}`, "#ef5350");
    for (let j = 0; j < levels.take_profits.length; j++) {
      const tp = levels.take_profits[j]!;
      const tag = tp.id.includes("1618") ? "TP*" : `TP${j + 1}`;
      push(tp.price, `${tag}: ${fmtPrice(tp.price)}`, "#29b6f6");
    }
  }

  return specs;
}

/** Latest future UNIX time in trade-level specs (for time-scale right offset). */
export function maxUnixTimeFromTradeLevelSpecs(specs: TradeLevelLineSpec[]): number | null {
  let m: number | null = null;
  for (const s of specs) {
    for (const p of s.segment) {
      const t = p.time as number;
      if (!Number.isFinite(t)) continue;
      if (m == null || t > m) m = t;
    }
  }
  return m;
}
