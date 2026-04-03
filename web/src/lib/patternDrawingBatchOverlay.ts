import type { SeriesMarker, UTCTimestamp } from "lightweight-charts";
import type { ChannelSixResponse, PatternDrawingBatchJson, PatternDrawingCommandJson } from "../api/client";
import type { ChartOhlcRow } from "./marketBarsToCandles";
import { filterDrawingBatchForDisplay, type AcpDisplay } from "./acpChartPatternsConfig";

export type PatternBatchOverlay = {
  channel: {
    upper: { time: UTCTimestamp; value: number }[];
    lower: { time: UTCTimestamp; value: number }[];
  } | null;
  zigzag: { time: UTCTimestamp; value: number }[];
  pivotLabels: SeriesMarker<UTCTimestamp>[];
  patternLabels: SeriesMarker<UTCTimestamp>[];
};

function rowTime(row: ChartOhlcRow): UTCTimestamp | null {
  const t = Math.floor(new Date(row.open_time).getTime() / 1000);
  return Number.isFinite(t) ? (t as UTCTimestamp) : null;
}

/** Pine `LineWrapper.get_price` — same formula as Rust `line_price_at_bar_index`. */
function linePriceAtBarIndex(
  p1Bar: number,
  p1Price: number,
  p2Bar: number,
  p2Price: number,
  bar: number,
): number | null {
  const d = p2Bar - p1Bar;
  if (d === 0) return null;
  return p1Price + ((bar - p1Bar) * (p2Price - p1Price)) / d;
}

function chartOpenTimeLookup(chartChrono: ChartOhlcRow[]): Map<string, ChartOhlcRow> {
  const m = new Map<string, ChartOhlcRow>();
  for (const r of chartChrono) {
    m.set(r.open_time, r);
  }
  return m;
}

function toPoint(
  /** Tarama API’sine gönderilen sıralı mum dilimi (`bar_index` 0 = bu dizinin ilk satırı). */
  scanBarsChrono: ChartOhlcRow[],
  p: { time_ms: number; price: number; bar_index?: number },
  /** When set, use the chart row with the same `open_time` as the scan row so LWC times match candle `setData` exactly. */
  chartByTime: Map<string, ChartOhlcRow> | null,
): { time: UTCTimestamp; value: number } | null {
  if (typeof p.bar_index === "number" && p.bar_index >= 0 && p.bar_index < scanBarsChrono.length) {
    const scanRow = scanBarsChrono[Math.floor(p.bar_index)]!;
    const row = chartByTime?.get(scanRow.open_time) ?? scanRow;
    const t = rowTime(row);
    if (t != null) return { time: t, value: p.price };
  }
  // Yedek: Rust `time_ms = bar_index * 60_000` yalnızca 1 dk bar uzayı için anlamlı; mümkünse bar_index ile hizalı dilim kullanın.
  const t = Math.floor(p.time_ms / 1000);
  return Number.isFinite(t) ? ({ time: t as UTCTimestamp, value: p.price } as const) : null;
}

function trendLineToSortedPair(
  cmd: Extract<PatternDrawingCommandJson, { kind: "trend_line" }>,
  scanBars: ChartOhlcRow[],
  chartByTime: Map<string, ChartOhlcRow> | null,
): [{ time: UTCTimestamp; value: number }, { time: UTCTimestamp; value: number }] | null {
  const ext = cmd.extend ?? "none";
  const extBars = Math.max(0, Math.floor(cmd.extend_bars ?? 0));
  const useExtend =
    extBars > 0 && (ext === "both" || ext === "left" || ext === "right") && scanBars.length > 0;

  if (
    !useExtend ||
    typeof cmd.p1.bar_index !== "number" ||
    typeof cmd.p2.bar_index !== "number"
  ) {
    const a = toPoint(scanBars, cmd.p1, chartByTime);
    const b = toPoint(scanBars, cmd.p2, chartByTime);
    if (!a || !b) return null;
    return a.time <= b.time ? [a, b] : [b, a];
  }

  const i1 = Math.floor(cmd.p1.bar_index);
  const i2 = Math.floor(cmd.p2.bar_index);
  const pr1 = cmd.p1.price;
  const pr2 = cmd.p2.price;
  let left = Math.min(i1, i2);
  let right = Math.max(i1, i2);
  if (ext === "both" || ext === "left") left = Math.max(0, left - extBars);
  if (ext === "both" || ext === "right") right = Math.min(scanBars.length - 1, right + extBars);

  const yL = linePriceAtBarIndex(i1, pr1, i2, pr2, left);
  const yR = linePriceAtBarIndex(i1, pr1, i2, pr2, right);
  if (yL == null || yR == null) return null;

  const pL = toPoint(
    scanBars,
    { time_ms: cmd.p1.time_ms, price: yL, bar_index: left },
    chartByTime,
  );
  const pR = toPoint(
    scanBars,
    { time_ms: cmd.p2.time_ms, price: yR, bar_index: right },
    chartByTime,
  );
  if (!pL || !pR) return null;
  return pL.time <= pR.time ? [pL, pR] : [pR, pL];
}

export function patternDrawingBatchToOverlay(
  barsChrono: ChartOhlcRow[],
  batch: PatternDrawingBatchJson | undefined,
  /** Full chart series (chrono); optional but recommended so line times match candlestick `time` keys after live updates. */
  chartBarsChrono?: ChartOhlcRow[] | null,
): PatternBatchOverlay | null {
  if (!batch || !barsChrono.length) return null;
  const chartByTime =
    chartBarsChrono?.length && chartBarsChrono.length > 0
      ? chartOpenTimeLookup(chartBarsChrono)
      : null;
  const trend: Array<{ time: UTCTimestamp; value: number }[]> = [];
  let zigzag: { time: UTCTimestamp; value: number }[] = [];
  const pivotLabels: SeriesMarker<UTCTimestamp>[] = [];
  const patternLabels: SeriesMarker<UTCTimestamp>[] = [];

  for (const cmd of batch.commands) {
    if (cmd.kind === "trend_line") {
      const pair = trendLineToSortedPair(cmd, barsChrono, chartByTime);
      if (pair) trend.push(pair);
      continue;
    }
    if (cmd.kind === "zigzag_polyline") {
      zigzag = cmd.points
        .map((p) => toPoint(barsChrono, p, chartByTime))
        .filter((x): x is { time: UTCTimestamp; value: number } => !!x);
      zigzag.sort((x, y) => (x.time as number) - (y.time as number));
      continue;
    }
    if (cmd.kind === "pivot_label") {
      const p = toPoint(barsChrono, cmd.at, chartByTime);
      if (!p) continue;
      const pos =
        cmd.anchor === "low" ? "belowBar" : cmd.anchor === "high" ? "aboveBar" : "inBar";
      pivotLabels.push({
        time: p.time,
        position: pos,
        shape: "circle",
        color: cmd.color_hex ?? "#b0bec5",
        text: cmd.text,
      });
      continue;
    }
    if (cmd.kind === "pattern_label") {
      const p = toPoint(barsChrono, cmd.at, chartByTime);
      if (!p) continue;
      patternLabels.push({
        time: p.time,
        position: "aboveBar",
        shape: "square",
        color: cmd.color_hex ?? "#ffd54f",
        text: cmd.text,
      });
    }
  }

  const channel = trend.length >= 2 ? { upper: trend[0], lower: trend[1] } : null;
  return { channel, zigzag, pivotLabels, patternLabels };
}

/** Zigzag çizim türü — TvChartPane Elliott / ACP ayrımı için. */
export type ZigzagLayerKind =
  | "default"
  | "elliott_abc"
  | "elliott_abc_sub"
  | "elliott_v2_macro"
  | "elliott_v2_intermediate"
  | "elliott_v2_micro"
  /** Post–P5 düzeltme (+a/+b/+c); grafikte her zaman kesik çizgi (tamamlanmamış öngörü). */
  | "elliott_v2_post_abc"
  | "elliott_v2_zigzag_macro"
  | "elliott_v2_zigzag_intermediate"
  | "elliott_v2_zigzag_micro"
  | "elliott_v2_hist_macro"
  | "elliott_v2_hist_intermediate"
  | "elliott_v2_hist_micro"
  /** Pine tarzı Fib şeması ileri projeksiyon (tahmin değildir). */
  | "elliott_projection"
  /** İkinci senaryo — örn. daha uzun 3. dalga hedefi (birincil projeksiyondan daha soluk). */
  | "elliott_projection_alt"
  /** Formasyon projeksiyonu hedef yatay seviyeleri (A/B/C gibi) — ayrı stil/legend için. */
  | "elliott_projection_target"
  /** İkinci senaryonun hedef yatay seviyeleri. */
  | "elliott_projection_target_alt"
  /** Projeksiyon: gerçekleşmiş/teyitli bölüm (düz çizgi). */
  | "elliott_projection_done"
  /** Projeksiyon: aktif C bacağı (kesik). */
  | "elliott_projection_c_active"
  /** DB motoru `trading_range` orta hat. */
  | "trading_range_mid"
  /** DB `range_signal_events` — türetilmiş açık long giriş seviyesi. */
  | "range_position_long"
  /** DB `range_signal_events` — türetilmiş açık short giriş seviyesi. */
  | "range_position_short";

/** Tek formasyon: üst/alt çizgi + zigzag (aynı indeks bileşende hizalı). */
export type PatternLayerOverlay = {
  upper: { time: UTCTimestamp; value: number }[];
  lower: { time: UTCTimestamp; value: number }[];
  zigzag: { time: UTCTimestamp; value: number }[];
  /** Yoksa klasik kesik zigzag (ACP). */
  zigzagKind?: ZigzagLayerKind;
  /** Varsa `zigzagLineOptions` rengini bununla geçersiz kılar (Elliott MTF menü rengi). */
  zigzagLineColor?: string;
  /** Varsa zigzag çizgi tipi (`solid` | `dotted` | `dashed`). */
  zigzagLineStyle?: "solid" | "dotted" | "dashed";
  /** Varsa zigzag çizgi kalınlığı. */
  zigzagLineWidth?: number;
  /**
   * Bu katmanın çizgi serisinde gösterilecek işaretler (ör. elliott_projection — mum yoksa zamanlarda
   * candlestick yerine zigzag hattında gösterilir).
   */
  zigzagMarkers?: SeriesMarker<UTCTimestamp>[];
};

/** TvChartPane — çoklu formasyon katmanları. */
export type MultiPatternChartOverlay = {
  layers: PatternLayerOverlay[];
  pivotLabels: SeriesMarker<UTCTimestamp>[];
  patternLabels: SeriesMarker<UTCTimestamp>[];
};

/** Aynı mumda aynı metin/yön — `pattern_matches` içinde çakışan pencereler üst üste binmesin. */
function markerDedupeKey(m: SeriesMarker<UTCTimestamp>): string {
  const pos = typeof m.position === "string" ? m.position : String(m.position ?? "");
  return `${m.time as number}\0${m.text ?? ""}\0${pos}`;
}

function sortMarkersByTime(markers: SeriesMarker<UTCTimestamp>[]): SeriesMarker<UTCTimestamp>[] {
  return [...markers].sort((a, b) => (a.time as number) - (b.time as number));
}

function acpLayerFingerprint(layer: {
  upper: { time: UTCTimestamp; value: number }[];
  lower: { time: UTCTimestamp; value: number }[];
  zigzag: { time: UTCTimestamp; value: number }[];
}): string {
  const seg = (pts: { time: UTCTimestamp; value: number }[]) =>
    pts.map((p) => `${p.time as number}:${p.value.toFixed(6)}`).join(";");
  return `${seg(layer.zigzag)}|${seg(layer.upper)}|${seg(layer.lower)}`;
}

/**
 * `pattern_matches` veya tek `pattern_drawing_batch` → birleşik çizim (ACP görünüm bayraklarına göre süzülür).
 * `barsChrono`, tarama isteğindeki mumlarla **aynı sıra ve uzunluk** olmalı: genelde `sorted.slice(-res.bar_count)`.
 */
export function buildMultiPatternOverlayFromScan(
  res: ChannelSixResponse | null,
  barsChrono: ChartOhlcRow[],
  display: AcpDisplay,
  /** Same as `patternDrawingBatchToOverlay` — full chart OHLC for `open_time` alignment with LWC candles. */
  chartBarsChrono?: ChartOhlcRow[] | null,
): MultiPatternChartOverlay | null {
  if (!res?.matched || !barsChrono.length) return null;

  const payloads =
    res.pattern_matches?.length && res.pattern_matches.length > 0
      ? res.pattern_matches
      : res.outcome && res.pattern_drawing_batch
        ? [
            {
              outcome: res.outcome,
              pattern_name: res.pattern_name,
              pattern_drawing_batch: res.pattern_drawing_batch,
            },
          ]
        : [];

  if (!payloads.length) return null;

  const layers: PatternLayerOverlay[] = [];
  const pivotLabels: SeriesMarker<UTCTimestamp>[] = [];
  const patternLabels: SeriesMarker<UTCTimestamp>[] = [];
  const pivotSeen = new Set<string>();
  const patternSeen = new Set<string>();
  const layerSeen = new Set<string>();

  for (const p of payloads) {
    const raw = p.pattern_drawing_batch;
    if (!raw) continue;
    const filtered = filterDrawingBatchForDisplay(raw, display);
    if (!filtered) continue;
    const o = patternDrawingBatchToOverlay(barsChrono, filtered, chartBarsChrono);
    if (!o) continue;
    for (const m of o.pivotLabels) {
      const k = markerDedupeKey(m);
      if (pivotSeen.has(k)) continue;
      pivotSeen.add(k);
      pivotLabels.push(m);
    }
    for (const m of o.patternLabels) {
      const k = markerDedupeKey(m);
      if (patternSeen.has(k)) continue;
      patternSeen.add(k);
      patternLabels.push(m);
    }
    const upper = o.channel?.upper ?? [];
    const lower = o.channel?.lower ?? [];
    const zigzag = o.zigzag ?? [];
    if (upper.length === 0 && lower.length === 0 && zigzag.length === 0) continue;
    const fp = acpLayerFingerprint({ upper, lower, zigzag });
    if (layerSeen.has(fp)) continue;
    layerSeen.add(fp);
    layers.push({ upper, lower, zigzag });
  }

  if (layers.length === 0 && pivotLabels.length === 0 && patternLabels.length === 0) {
    return null;
  }

  return {
    layers,
    pivotLabels: sortMarkersByTime(pivotLabels),
    patternLabels: sortMarkersByTime(patternLabels),
  };
}
