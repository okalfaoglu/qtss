import { chartOhlcRowsSortedChrono } from "./chartRowsToOhlcBars";
import type { ChartOhlcRow } from "./marketBarsToCandles";
import type { PatternLayerOverlay } from "./patternDrawingBatchOverlay";
import type { RangeSignalEventApiRow } from "../api/client";
import type { UTCTimestamp } from "lightweight-charts";

function rowTime(row: ChartOhlcRow): UTCTimestamp | null {
  const t = Math.floor(new Date(row.open_time).getTime() / 1000);
  return Number.isFinite(t) ? (t as UTCTimestamp) : null;
}

/** Olayları kronolojik sırala (aynı bar’da `created_at`). */
function sortSignalEvents(ev: RangeSignalEventApiRow[]): RangeSignalEventApiRow[] {
  return [...ev].sort((a, b) => {
    const ta = new Date(a.bar_open_time).getTime();
    const tb = new Date(b.bar_open_time).getTime();
    if (ta !== tb) return ta - tb;
    return new Date(a.created_at).getTime() - new Date(b.created_at).getTime();
  });
}

/**
 * `range_signal_events` zincirinden türetilen açık yön ve giriş fiyatı.
 * Gerçek borsa pozisyonu değil; yalnızca motor olaylarının sonucu.
 */
export function deriveOpenPositionFromRangeEvents(
  events: RangeSignalEventApiRow[],
): { side: "long" | "short"; entryPrice: number } | null {
  let side: "long" | "short" | "flat" = "flat";
  let entryPrice = 0;
  for (const ev of sortSignalEvents(events)) {
    const px = ev.reference_price;
    if (px == null || !Number.isFinite(px)) continue;
    switch (ev.event_kind) {
      case "long_entry":
        side = "long";
        entryPrice = px;
        break;
      case "long_exit":
        if (side === "long") side = "flat";
        break;
      case "short_entry":
        side = "short";
        entryPrice = px;
        break;
      case "short_exit":
        if (side === "short") side = "flat";
        break;
      default:
        break;
    }
  }
  if (side === "flat") return null;
  return { side, entryPrice };
}

/** Açık pozisyon varsa grafik genişliğinde yatay çizgi (`zigzag` serisi). */
export function openPositionLayerFromRangeEvents(
  bars: ChartOhlcRow[],
  events: RangeSignalEventApiRow[],
): PatternLayerOverlay | null {
  const open = deriveOpenPositionFromRangeEvents(events);
  if (!open) return null;
  const ch = chartOhlcRowsSortedChrono(bars);
  if (!ch.length) return null;
  const t0 = rowTime(ch[0]!);
  const t1 = rowTime(ch[ch.length - 1]!);
  if (t0 == null || t1 == null) return null;
  const { side, entryPrice } = open;
  return {
    upper: [],
    lower: [],
    zigzag: [
      { time: t0, value: entryPrice },
      { time: t1, value: entryPrice },
    ],
    zigzagKind: side === "long" ? "range_position_long" : "range_position_short",
    zigzagLineStyle: "dashed",
    zigzagLineWidth: 2,
  };
}
