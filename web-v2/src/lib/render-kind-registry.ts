// Aşama 5 — migration-tabanlı overlay katmanı dispatch tablosu.
//
// Detector explicit `render_geometry: { kind, payload }` emit ettiyse
// Chart.tsx buraya dispatch eder; yoksa legacy anchor-derived çizime
// düşer. Yeni render kind eklemek = bir `draw*` fn yaz + registry'ye bir
// satır ekle; Chart.tsx'te match arm yok (CLAUDE.md #1).
//
// Aşama 5.B — 11 render kind'ın hepsi live:
//   polyline, two_lines, horizontal_band, head_shoulders, double_pattern,
//   arc, v_spike, gap_marker, candle_annotation, diamond, fibonacci_ruler

import type { ISeriesApi, IChartApi, SeriesMarker, Time } from "lightweight-charts";
import { LineSeries, LineStyle } from "lightweight-charts";

import { RectanglePrimitive, type RectangleOptions } from "./rectangle-primitive";
import { PolygonPrimitive, type PolygonOptions } from "./polygon-primitive";

/** Side-effect sinks — Chart.tsx re-uses its existing arrays so cleanup
 *  runs through one path. */
export interface RenderSinks {
  rects: RectanglePrimitive[];
  lines: ISeriesApi<"Line">[];
  markers: SeriesMarker<Time>[];
  polygons?: PolygonPrimitive[];
}

export interface RenderContext {
  chart: IChartApi;
  candleSeries: ISeriesApi<"Candlestick">;
  sinks: RenderSinks;
  isoToUnix: (iso: string) => Time;
  familyColor: string;
  styleKey: string | null;
  faded: boolean;
}

export type RenderKind =
  | "polyline"
  | "two_lines"
  | "horizontal_band"
  | "head_shoulders"
  | "double_pattern"
  | "harmonic"
  | "arc"
  | "v_spike"
  | "gap_marker"
  | "candle_annotation"
  | "diamond"
  | "fibonacci_ruler";

export type RenderDrawFn = (payload: unknown, ctx: RenderContext) => void;

/** Aşama 5.C — mobile viewport heuristic. Keeps the chart readable on
 *  narrow screens by reducing geometry sample density. SSR-safe. */
function isCompactViewport(): boolean {
  return typeof window !== "undefined" && window.innerWidth < 640;
}

// ── Shared primitives ─────────────────────────────────────────────────

interface Point {
  time: string;
  price: number | string;
  label?: string;
}

function toLineData(points: Point[], ctx: RenderContext) {
  // lightweight-charts requires data sorted strictly ascending by time
  // and silently rejects series that violate this. Some detectors
  // (Elliott impulse, harmonic XABCD, anything with labelled legs) emit
  // anchors in pivot / pattern order, which is NOT always time-order.
  // Earlier we dedup'd consecutive duplicates only and passed through
  // whatever order the backend sent — that is why Elliott impulse
  // detections showed markers but no connecting polyline: setData
  // rejected the out-of-order array.
  //
  // Sort first (stable), then drop equal-time duplicates (lightweight-
  // charts forbids equal times too). The sort is cheap (≤ dozen points)
  // and time-ordering is universally correct for every line overlay we
  // draw.
  const mapped = points.map((p) => ({
    time: ctx.isoToUnix(p.time),
    value: Number(p.price),
  }));
  mapped.sort((a, b) => Number(a.time) - Number(b.time));
  return dedupe(mapped);
}

/** Drop duplicates on `time` — lightweight-charts rejects equal
 *  timestamps. Called after sort, so we only need a single pass. */
function dedupe<T extends { time: Time }>(data: T[]): T[] {
  const out: T[] = [];
  let last: unknown = null;
  for (const d of data) {
    if (d.time !== last) {
      out.push(d);
      last = d.time;
    }
  }
  return out;
}

function pushLine(
  ctx: RenderContext,
  points: Point[],
  opts: { width?: number; style?: LineStyle; color?: string } = {},
) {
  if (points.length < 2) return;
  const series = ctx.chart.addSeries(LineSeries, {
    color: opts.color ?? ctx.familyColor,
    lineWidth: (opts.width ?? (ctx.faded ? 1 : 2)) as 1 | 2 | 3 | 4,
    lineStyle: opts.style ?? (ctx.faded ? LineStyle.Dotted : LineStyle.Solid),
    crosshairMarkerVisible: false,
    lastValueVisible: false,
    priceLineVisible: false,
    pointMarkersVisible: true,
    pointMarkersRadius: 3,
  });
  series.setData(toLineData(points, ctx));
  ctx.sinks.lines.push(series);
  // Anchor-label markers are drawn by Chart.tsx's second-pass marker
  // loop (one-size-fits-all positioning, elliott-degree remapping,
  // confidence chip). Pushing them here too produces stacked duplicate
  // labels (X/X, A/A, B/B … on harmonic; i/[i] on Elliott). Intentional
  // no-op.
}

function pushRect(ctx: RenderContext, o: RectangleOptions) {
  const prim = new RectanglePrimitive(o);
  ctx.candleSeries.attachPrimitive(prim);
  ctx.sinks.rects.push(prim);
}

function pushPolygon(ctx: RenderContext, o: PolygonOptions) {
  const prim = new PolygonPrimitive(o);
  ctx.candleSeries.attachPrimitive(prim);
  // Tracked in the polygons sink when available, otherwise piggy-backs
  // on the rects sink — both primitive types share the same
  // detachPrimitive() cleanup call site in Chart.tsx.
  if (ctx.sinks.polygons) {
    ctx.sinks.polygons.push(prim);
  } else {
    (ctx.sinks.rects as unknown as PolygonPrimitive[]).push(prim);
  }
}

// ── Kind implementations ──────────────────────────────────────────────

/** `{ points: [{time, price, label?}, ...] }` — single polyline. */
const drawPolyline: RenderDrawFn = (payload, ctx) => {
  const p = payload as { points?: Point[] };
  if (!p.points?.length) return;
  pushLine(ctx, p.points);
};

/** `{ upper: Point[], lower: Point[] }` — two trendlines (triangles,
 *  wedges, channels, rectangles, broadenings). */
const drawTwoLines: RenderDrawFn = (payload, ctx) => {
  const p = payload as { upper?: Point[]; lower?: Point[] };
  if (p.upper) pushLine(ctx, p.upper);
  if (p.lower) pushLine(ctx, p.lower);
};

/** `{ time_start, time_end, price_low, price_high }` — opaque band. */
const drawHorizontalBand: RenderDrawFn = (payload, ctx) => {
  const p = payload as {
    time_start?: string;
    time_end?: string;
    price_low?: number | string;
    price_high?: number | string;
  };
  if (!p.time_start || !p.time_end || p.price_low == null || p.price_high == null)
    return;
  pushRect(ctx, {
    time1: ctx.isoToUnix(p.time_start),
    time2: ctx.isoToUnix(p.time_end),
    priceTop: Math.max(Number(p.price_low), Number(p.price_high)),
    priceBottom: Math.min(Number(p.price_low), Number(p.price_high)),
    fillColor: withAlpha(ctx.familyColor, ctx.faded ? 0.05 : 0.12),
    borderColor: withAlpha(ctx.familyColor, ctx.faded ? 0.25 : 0.6),
    borderWidth: 1,
  });
};

/**
 * `{ left_shoulder, head, right_shoulder, neck_left, neck_right }` —
 * five anchors. Renders:
 *   • polyline LS → H → RS (formation)
 *   • neckline neck_left → neck_right (extended dashed)
 */
const drawHeadShoulders: RenderDrawFn = (payload, ctx) => {
  const p = payload as {
    left_shoulder?: Point;
    head?: Point;
    right_shoulder?: Point;
    neck_left?: Point;
    neck_right?: Point;
  };
  if (!p.left_shoulder || !p.head || !p.right_shoulder) return;
  pushLine(ctx, [
    { ...p.left_shoulder, label: p.left_shoulder.label ?? "LS" },
    { ...p.head, label: p.head.label ?? "H" },
    { ...p.right_shoulder, label: p.right_shoulder.label ?? "RS" },
  ]);
  if (p.neck_left && p.neck_right) {
    pushLine(ctx, [p.neck_left, p.neck_right], {
      width: 1,
      style: LineStyle.Dashed,
    });
  }
};

/**
 * `{ peaks: [Point, Point], trough?: Point, neck?: number }` — double
 * top/bottom. Draws the two peaks + optional neckline as a horizontal
 * reference. `trough` connects them visually.
 */
/**
 * Double top (M) / double bottom (W).
 *
 * Anchors from detector: [extreme1, trough/peak, extreme2]. Drawing only
 * those 3 points yields a bare `\/` or `/\` — the middle of an M/W,
 * missing the outer legs, so users complained it does not look like a
 * proper M / W.
 *
 * Fix: synthesize a pre-leg and post-leg at the neck level so the full
 * shape is 5 points:
 *
 *   M (double top):    neck ─ /\  ─ \/ ─ /\  ─ neck
 *                             p1   trough  p2
 *   W (double bottom): neck ─ \/  ─ /\ ─ \/  ─ neck
 *                             t1    peak   t2
 *
 * Leg width = gap between the middle (trough/peak) and extreme2, so the
 * silhouette is symmetric. Neckline dashed line is also extended to span
 * the full M/W width.
 */
const drawDoublePattern: RenderDrawFn = (payload, ctx) => {
  const p = payload as {
    peaks?: [Point, Point];
    trough?: Point;
    neck?: number | string;
  };
  if (!p.peaks || p.peaks.length !== 2) return;

  const e1 = p.peaks[0];
  const e2 = p.peaks[1];
  const mid = p.trough;

  if (!mid) {
    // Degenerate — detector gave only two extremes, no neck. Fall back
    // to straight line; nothing M/W-shaped possible.
    pushLine(ctx, [
      { ...e1, label: e1.label ?? "1" },
      { ...e2, label: e2.label ?? "2" },
    ]);
    return;
  }

  const neckPrice = p.neck != null ? Number(p.neck) : Number(mid.price);
  const peak1Price = Number(e1.price);
  const peak2Price = Number(e2.price);
  const troughPrice = Number(mid.price);
  const t1 = Date.parse(e1.time);
  const tMid = Date.parse(mid.time);
  const t2 = Date.parse(e2.time);
  if (!isFinite(t1) || !isFinite(tMid) || !isFinite(t2)) return;

  // Letter-M / W geometry. Four equal strokes, middle valley at ~50% of
  // peak-to-baseline amplitude. Previous version terminated outer legs
  // at neck level, which made the shape look like two adjacent triangles
  // (the middle V reached the same baseline as the outer starts). A
  // real letter M keeps the middle dip shallower than the outer tails.
  //
  //   baseline  = trough − 0.5 × (peakAvg − trough)        for double top
  //             = trough + 0.5 × (trough  − peakAvg)       for double bottom
  //   (signed `h` handles both symmetrically)
  const peakAvg = (peak1Price + peak2Price) / 2;
  const h = peakAvg - troughPrice; // +ve double top, −ve double bottom
  const outerPrice = troughPrice - h * 0.5;

  // Symmetric leg width — use the wider inner span so legs stay in
  // proportion even when the detector's peaks/trough are time-skewed.
  const legMs = Math.max(Math.abs(tMid - t1), Math.abs(t2 - tMid));
  const preIso = new Date(t1 - legMs).toISOString();
  const postIso = new Date(t2 + legMs).toISOString();

  const fullPath: Point[] = [
    { time: preIso, price: outerPrice },
    { ...e1, label: e1.label ?? "1" },
    { ...mid, label: mid.label ?? "N" },
    { ...e2, label: e2.label ?? "2" },
    { time: postIso, price: outerPrice },
  ];
  pushLine(ctx, fullPath);

  // Dashed neckline at the REAL neck (trough level) — the break level
  // users care about, not the synthetic outer baseline. Extended across
  // the full M/W width for visual anchoring.
  pushLine(
    ctx,
    [
      { time: preIso, price: neckPrice },
      { time: postIso, price: neckPrice },
    ],
    { width: 1, style: LineStyle.Dashed },
  );
};

/**
 * `{ xabcd: [X, A, B, C, D] }` — harmonic Gartley/Bat/Butterfly/Crab/
 * Shark/Cypher. Rendered the way classical TA charting software draws
 * them: two filled triangles (X-A-B and B-C-D) plus the skeleton
 * polyline X→A→B→C→D and a dashed X→D reference chord. Fib ratio
 * labels (e.g. "0,618", "1,272") can ride on the polyline labels.
 *
 * Why two triangles and not one polygon: the correct harmonic visual
 * has the two "wings" (AB-leg down, CD-leg down in a bullish pattern)
 * filled independently so the shape reads as two connected lobes. A
 * single pentagon fill would smear across the B pivot and lose the
 * pattern's visual identity.
 */
const drawHarmonic: RenderDrawFn = (payload, ctx) => {
  const p = payload as {
    xabcd?: Point[];
  };
  const pts = p.xabcd;
  if (!pts || pts.length < 5) return;

  const [X, A, B, C, D] = pts;
  const toVertex = (pt: Point) => ({
    time: ctx.isoToUnix(pt.time),
    price: Number(pt.price),
  });
  const fill = withAlpha(ctx.familyColor, ctx.faded ? 0.06 : 0.18);
  const border = withAlpha(ctx.familyColor, ctx.faded ? 0.3 : 0.55);

  // Triangle 1 — X-A-B (left wing).
  pushPolygon(ctx, {
    vertices: [toVertex(X), toVertex(A), toVertex(B)],
    fillColor: fill,
    borderColor: border,
    borderWidth: 0,
  });
  // Triangle 2 — B-C-D (right wing).
  pushPolygon(ctx, {
    vertices: [toVertex(B), toVertex(C), toVertex(D)],
    fillColor: fill,
    borderColor: border,
    borderWidth: 0,
  });

  // Skeleton polyline X → A → B → C → D with letter labels.
  pushLine(ctx, [
    { ...X, label: X.label ?? "X" },
    { ...A, label: A.label ?? "A" },
    { ...B, label: B.label ?? "B" },
    { ...C, label: C.label ?? "C" },
    { ...D, label: D.label ?? "D" },
  ]);

  // Dashed X-D reference chord (the pattern's base).
  pushLine(
    ctx,
    [
      { time: X.time, price: X.price },
      { time: D.time, price: D.price },
    ],
    { width: 1, style: LineStyle.Dashed },
  );
};

/**
 * `{ start, apex, end, curvature? }` — rounding top/bottom arc. Since
 * lightweight-charts has no curve primitive we sample an N-point
 * parabola through (start, apex, end). Quadratic Lagrange.
 */
const drawArc: RenderDrawFn = (payload, ctx) => {
  const p = payload as { start?: Point; apex?: Point; end?: Point };
  if (!p.start || !p.apex || !p.end) return;
  const t0 = Number(ctx.isoToUnix(p.start.time));
  const t1 = Number(ctx.isoToUnix(p.apex.time));
  const t2 = Number(ctx.isoToUnix(p.end.time));
  const y0 = Number(p.start.price);
  const y1 = Number(p.apex.price);
  const y2 = Number(p.end.price);
  // Arc sample density — halve on narrow viewports so a rounding
  // top/bottom stays smooth without burning off-screen lines.
  const N = isCompactViewport() ? 12 : 32;
  const pts: Point[] = [];
  for (let i = 0; i <= N; i++) {
    const t = t0 + ((t2 - t0) * i) / N;
    const L0 = ((t - t1) * (t - t2)) / ((t0 - t1) * (t0 - t2));
    const L1 = ((t - t0) * (t - t2)) / ((t1 - t0) * (t1 - t2));
    const L2 = ((t - t0) * (t - t1)) / ((t2 - t0) * (t2 - t1));
    const y = y0 * L0 + y1 * L1 + y2 * L2;
    pts.push({ time: new Date(t * 1000).toISOString(), price: y });
  }
  pushLine(ctx, pts, { width: 2 });
};

/**
 * `{ pre, spike, post }` — V reversal. Draws a 3-point polyline with
 * the middle apex emphasized via a shape marker.
 */
const drawVSpike: RenderDrawFn = (payload, ctx) => {
  const p = payload as { pre?: Point; spike?: Point; post?: Point };
  if (!p.pre || !p.spike || !p.post) return;
  pushLine(ctx, [p.pre, p.spike, p.post], { width: 2 });
  const bull = Number(p.spike.price) < Number(p.pre.price); // V-bottom
  ctx.sinks.markers.push({
    time: ctx.isoToUnix(p.spike.time),
    position: bull ? "belowBar" : "aboveBar",
    color: ctx.familyColor,
    shape: bull ? "arrowUp" : "arrowDown",
    text: p.spike.label ?? "V",
  });
};

/** `{ time, price, direction: "bull"|"bear", label? }` — single arrow. */
const drawCandleAnnotation: RenderDrawFn = (payload, ctx) => {
  const p = payload as { time?: string; direction?: string; label?: string };
  if (!p.time) return;
  const bull = p.direction !== "bear";
  ctx.sinks.markers.push({
    time: ctx.isoToUnix(p.time),
    position: bull ? "belowBar" : "aboveBar",
    color: ctx.familyColor,
    shape: bull ? "arrowUp" : "arrowDown",
    text: p.label,
  });
};

/** `{ time, time_end?, price_pre, price_post }` — gap band. */
const drawGapMarker: RenderDrawFn = (payload, ctx) => {
  const p = payload as {
    time?: string;
    time_end?: string;
    price_pre?: number | string;
    price_post?: number | string;
  };
  if (!p.time || p.price_pre == null || p.price_post == null) return;
  const t1 = ctx.isoToUnix(p.time);
  const t2 = p.time_end ? ctx.isoToUnix(p.time_end) : ((Number(t1) + 60) as Time);
  pushRect(ctx, {
    time1: t1,
    time2: t2,
    priceTop: Math.max(Number(p.price_pre), Number(p.price_post)),
    priceBottom: Math.min(Number(p.price_pre), Number(p.price_post)),
    fillColor: withAlpha(ctx.familyColor, 0.18),
    borderColor: withAlpha(ctx.familyColor, 0.7),
    borderWidth: 1,
  });
};

/**
 * `{ top, bottom, left, right }` — 4 anchors of a diamond. Draws the
 * four edges as one closed polyline.
 */
const drawDiamond: RenderDrawFn = (payload, ctx) => {
  const p = payload as {
    top?: Point;
    right?: Point;
    bottom?: Point;
    left?: Point;
  };
  if (!p.top || !p.right || !p.bottom || !p.left) return;
  // Walk in temporal order so the polyline doesn't cross itself on the
  // chart: left → top → right → bottom (assumes left earliest, right
  // latest). The detector is expected to pass them in that order; we
  // still sort to be safe.
  const ordered = [p.left, p.top, p.right, p.bottom].sort(
    (a, b) => Number(ctx.isoToUnix(a.time)) - Number(ctx.isoToUnix(b.time)),
  );
  pushLine(ctx, [...ordered, ordered[0]]);
};

/**
 * `{ base: Point, target: Point, ratios?: number[] }` — Fibonacci
 * ruler. Horizontal lines at base + (target-base)*r for each ratio.
 * Defaults: 0, 0.382, 0.5, 0.618, 1.0, 1.272, 1.618.
 */
const drawFibonacciRuler: RenderDrawFn = (payload, ctx) => {
  const p = payload as {
    base?: Point;
    target?: Point;
    ratios?: number[];
  };
  if (!p.base || !p.target) return;
  // Compact viewport: drop the inner 0.382/0.5/1.272 to keep the ruler
  // readable; golden ratios (0, 0.618, 1.0, 1.618) survive.
  const ratios =
    p.ratios ??
    (isCompactViewport()
      ? [0, 0.618, 1.0, 1.618]
      : [0, 0.382, 0.5, 0.618, 1.0, 1.272, 1.618]);
  const basePrice = Number(p.base.price);
  const targetPrice = Number(p.target.price);
  const t1 = p.base.time;
  const t2 = p.target.time;
  for (const r of ratios) {
    const y = basePrice + (targetPrice - basePrice) * r;
    pushLine(
      ctx,
      [
        { time: t1, price: y, label: `${(r * 100).toFixed(1)}%` },
        { time: t2, price: y },
      ],
      { width: 1, style: LineStyle.Dotted },
    );
  }
};

// ── Registry dispatch table (CLAUDE.md #1) ────────────────────────────

export const RENDER_KIND_REGISTRY: Record<RenderKind, RenderDrawFn> = {
  polyline: drawPolyline,
  two_lines: drawTwoLines,
  horizontal_band: drawHorizontalBand,
  head_shoulders: drawHeadShoulders,
  double_pattern: drawDoublePattern,
  harmonic: drawHarmonic,
  arc: drawArc,
  v_spike: drawVSpike,
  gap_marker: drawGapMarker,
  candle_annotation: drawCandleAnnotation,
  diamond: drawDiamond,
  fibonacci_ruler: drawFibonacciRuler,
};

/** Returns true when the overlay was rendered via the registry and the
 *  caller should skip the legacy anchor path. */
export function dispatchRenderGeometry(
  geometry: { kind: string; payload: unknown } | null | undefined,
  ctx: RenderContext,
): boolean {
  if (!geometry || !geometry.kind) return false;
  const draw = RENDER_KIND_REGISTRY[geometry.kind as RenderKind];
  if (!draw) return false;
  draw(geometry.payload, ctx);
  return true;
}

// ── Helpers ───────────────────────────────────────────────────────────

function withAlpha(hex: string, alpha: number): string {
  const m = /^#([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return hex;
  const n = parseInt(m[1], 16);
  const r = (n >> 16) & 0xff;
  const g = (n >> 8) & 0xff;
  const b = n & 0xff;
  return `rgba(${r}, ${g}, ${b}, ${alpha.toFixed(3)})`;
}
