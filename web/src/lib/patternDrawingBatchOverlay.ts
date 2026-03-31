import type { SeriesMarker, UTCTimestamp } from "lightweight-charts";
import type { ChannelSixResponse, PatternDrawingBatchJson } from "../api/client";
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

function toPoint(
  /** Tarama API’sine gönderilen sıralı mum dilimi (`bar_index` 0 = bu dizinin ilk satırı). */
  barsChrono: ChartOhlcRow[],
  p: { time_ms: number; price: number; bar_index?: number },
): { time: UTCTimestamp; value: number } | null {
  if (typeof p.bar_index === "number" && p.bar_index >= 0 && p.bar_index < barsChrono.length) {
    const t = rowTime(barsChrono[p.bar_index]);
    if (t != null) return { time: t, value: p.price };
  }
  // Yedek: Rust `time_ms = bar_index * 60_000` yalnızca 1 dk bar uzayı için anlamlı; mümkünse bar_index ile hizalı dilim kullanın.
  const t = Math.floor(p.time_ms / 1000);
  return Number.isFinite(t) ? ({ time: t as UTCTimestamp, value: p.price } as const) : null;
}

export function patternDrawingBatchToOverlay(
  barsChrono: ChartOhlcRow[],
  batch: PatternDrawingBatchJson | undefined,
): PatternBatchOverlay | null {
  if (!batch || !barsChrono.length) return null;
  const trend: Array<{ time: UTCTimestamp; value: number }[]> = [];
  let zigzag: { time: UTCTimestamp; value: number }[] = [];
  const pivotLabels: SeriesMarker<UTCTimestamp>[] = [];
  const patternLabels: SeriesMarker<UTCTimestamp>[] = [];

  for (const cmd of batch.commands) {
    if (cmd.kind === "trend_line") {
      const a = toPoint(barsChrono, cmd.p1);
      const b = toPoint(barsChrono, cmd.p2);
      if (a && b) trend.push(a.time <= b.time ? [a, b] : [b, a]);
      continue;
    }
    if (cmd.kind === "zigzag_polyline") {
      zigzag = cmd.points.map((p) => toPoint(barsChrono, p)).filter((x): x is { time: UTCTimestamp; value: number } => !!x);
      zigzag.sort((x, y) => (x.time as number) - (y.time as number));
      continue;
    }
    if (cmd.kind === "pivot_label") {
      const p = toPoint(barsChrono, cmd.at);
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
      const p = toPoint(barsChrono, cmd.at);
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

function mergeSortedMarkers(
  acc: SeriesMarker<UTCTimestamp>[],
  more: SeriesMarker<UTCTimestamp>[],
): SeriesMarker<UTCTimestamp>[] {
  return [...acc, ...more].sort((x, y) => (x.time as number) - (y.time as number));
}

/**
 * `pattern_matches` veya tek `pattern_drawing_batch` → birleşik çizim (ACP görünüm bayraklarına göre süzülür).
 * `barsChrono`, tarama isteğindeki mumlarla **aynı sıra ve uzunluk** olmalı: genelde `sorted.slice(-res.bar_count)`.
 */
export function buildMultiPatternOverlayFromScan(
  res: ChannelSixResponse | null,
  barsChrono: ChartOhlcRow[],
  display: AcpDisplay,
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
  let pivotLabels: SeriesMarker<UTCTimestamp>[] = [];
  let patternLabels: SeriesMarker<UTCTimestamp>[] = [];

  for (const p of payloads) {
    const raw = p.pattern_drawing_batch;
    if (!raw) continue;
    const filtered = filterDrawingBatchForDisplay(raw, display);
    if (!filtered) continue;
    const o = patternDrawingBatchToOverlay(barsChrono, filtered);
    if (!o) continue;
    pivotLabels = mergeSortedMarkers(pivotLabels, o.pivotLabels);
    patternLabels = mergeSortedMarkers(patternLabels, o.patternLabels);
    const upper = o.channel?.upper ?? [];
    const lower = o.channel?.lower ?? [];
    const zigzag = o.zigzag ?? [];
    if (upper.length === 0 && lower.length === 0 && zigzag.length === 0) continue;
    layers.push({ upper, lower, zigzag });
  }

  if (layers.length === 0 && pivotLabels.length === 0 && patternLabels.length === 0) {
    return null;
  }

  return { layers, pivotLabels, patternLabels };
}
