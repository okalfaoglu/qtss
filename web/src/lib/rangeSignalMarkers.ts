import type { SeriesMarker, UTCTimestamp } from "lightweight-charts";
import { chartOhlcRowsSortedChrono } from "./chartRowsToOhlcBars";
import type { ChartOhlcRow } from "./marketBarsToCandles";
import type { RangeSignalEventApiRow } from "../api/client";

function barTimeSec(row: ChartOhlcRow): number | null {
  const t = Math.floor(new Date(row.open_time).getTime() / 1000);
  return Number.isFinite(t) ? t : null;
}

function eventBarTimeSec(ev: RangeSignalEventApiRow): number | null {
  const t = Math.floor(new Date(ev.bar_open_time).getTime() / 1000);
  return Number.isFinite(t) ? t : null;
}

function markerGeometry(
  kind: string,
): Pick<SeriesMarker<UTCTimestamp>, "position" | "shape" | "color"> {
  switch (kind) {
    case "long_entry":
      return { position: "belowBar", shape: "arrowUp", color: "#26a69a" };
    case "long_exit":
      return { position: "aboveBar", shape: "circle", color: "#ffca28" };
    case "short_entry":
      return { position: "aboveBar", shape: "arrowDown", color: "#ef5350" };
    case "short_exit":
      return { position: "belowBar", shape: "circle", color: "#ff9800" };
    default:
      return { position: "aboveBar", shape: "circle", color: "#9e9e9e" };
  }
}

function defaultMarkerText(kind: string): string {
  switch (kind) {
    case "long_entry":
      return "L Enter";
    case "long_exit":
      return "L Exit";
    case "short_entry":
      return "S Enter";
    case "short_exit":
      return "S Exit";
    default:
      return kind;
  }
}

/**
 * DB `range_signal_events` → mum serisi marker’ları.
 * Yalnızca grafikte yüklü bir mumun `open_time` ile eşleşen olaylar çizilir (zaman saniye bazında).
 * @param markerText — i18n üzerinden etiket (ör. `L Enter`); yoksa kısa İngilizce varsayılan.
 */
export function rangeSignalMarkersFromEvents(
  bars: ChartOhlcRow[],
  events: RangeSignalEventApiRow[],
  markerText?: (eventKind: string) => string,
): SeriesMarker<UTCTimestamp>[] {
  if (!bars.length || !events.length) return [];
  const ch = chartOhlcRowsSortedChrono(bars);
  const barTimes = new Set<number>();
  for (const r of ch) {
    const s = barTimeSec(r);
    if (s != null) barTimes.add(s);
  }

  const out: SeriesMarker<UTCTimestamp>[] = [];
  for (const ev of events) {
    const sec = eventBarTimeSec(ev);
    if (sec == null || !barTimes.has(sec)) continue;
    const geo = markerGeometry(ev.event_kind);
    const text = markerText ? markerText(ev.event_kind) : defaultMarkerText(ev.event_kind);
    out.push({
      time: sec as UTCTimestamp,
      position: geo.position,
      shape: geo.shape,
      color: geo.color,
      text,
    });
  }
  return out.sort((a, b) => (a.time as number) - (b.time as number));
}
