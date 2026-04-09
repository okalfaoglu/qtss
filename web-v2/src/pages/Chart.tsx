import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { CandleBar, ChartWorkspace, DetectionOverlay } from "../lib/types";

const DEFAULTS = { venue: "binance", symbol: "BTCUSDT", timeframe: "1h" };
const PAGE_SIZE = 500;
const DEFAULT_VIEW_SIZE = 200;
const MIN_VIEW_SIZE = 20;
const MAX_VIEW_SIZE = 2000;
const ZOOM_FACTOR = 1.25;
const PREFETCH_THRESHOLD = 50; // load older when within N candles of the left edge
const ZIGZAG_PCT = 0.01; // 1% reversal threshold for the on-chart zigzag overlay

// Layout constants. Kept here (single source of truth) so the SVG
// math below stays one place to edit if we ever resize the chart.
const W = 960;
const H = 420;
const PAD_L = 56;
const PAD_R = 16;
const PAD_T = 12;
const PAD_B = 36;
// Right-side fraction reserved for forward projections (Faz 7.6 / A2)
// so projected anchors don't get clamped to the chart edge.
const PROJ_FRAC = 0.18;
// Sub-wave (Faz 7.6 / A3) — distinct hue, intentionally not the
// detection's family color so the eye separates inner-degree pivots
// from the realized formation.
const SUBWAVE_COLOR = "rgb(250 204 21)"; // amber-400
// Volume overlay band — sits just above the time axis. Kept small
// (≈14% of inner height) and faint so candles above stay readable.
const VOL_BAND_H = 56;

interface PriceScale {
  min: number;
  max: number;
  toY: (price: number) => number;
}

function buildScale(candles: CandleBar[]): PriceScale {
  if (candles.length === 0) {
    return { min: 0, max: 1, toY: () => H / 2 };
  }
  let min = Infinity;
  let max = -Infinity;
  for (const c of candles) {
    const lo = Number(c.low);
    const hi = Number(c.high);
    if (lo < min) min = lo;
    if (hi > max) max = hi;
  }
  const span = max - min || 1;
  const innerH = H - PAD_T - PAD_B;
  return {
    min,
    max,
    toY: (p) => PAD_T + innerH - ((p - min) / span) * innerH,
  };
}

// Faint volume bars in a thin band at the bottom of the price area.
// Sized against the visible-window peak so the largest bar always
// touches the band ceiling — gives a sense of relative pressure
// without dictating absolute units. Drawn before candles so the
// candle bodies / wicks always overpaint.
function Volume({ candles }: { candles: CandleBar[] }) {
  if (candles.length === 0) return null;
  const innerW = W - PAD_L - PAD_R;
  const candleAreaW = innerW * (1 - PROJ_FRAC);
  const step = candleAreaW / candles.length;
  const bodyW = Math.max(1, step * 0.7);
  const bandTop = H - PAD_B - VOL_BAND_H;
  const bandBottom = H - PAD_B;
  let peak = 0;
  for (const c of candles) {
    const v = Number(c.volume) || 0;
    if (v > peak) peak = v;
  }
  if (peak <= 0) return null;
  return (
    <g>
      {/* Band baseline (subtle separator from candle area). */}
      <line
        x1={PAD_L}
        x2={PAD_L + candleAreaW}
        y1={bandTop}
        y2={bandTop}
        stroke="rgb(63 63 70)"
        strokeOpacity={0.3}
        strokeDasharray="2 4"
        strokeWidth={1}
      />
      {candles.map((c, i) => {
        const v = Number(c.volume) || 0;
        const h = (v / peak) * VOL_BAND_H;
        const x = PAD_L + i * step + step / 2;
        const up = Number(c.close) >= Number(c.open);
        const fill = up ? "rgb(52 211 153)" : "rgb(248 113 113)";
        return (
          <rect
            key={c.open_time}
            x={x - bodyW / 2}
            y={bandBottom - h}
            width={bodyW}
            height={Math.max(0.5, h)}
            fill={fill}
            fillOpacity={0.35}
          />
        );
      })}
    </g>
  );
}

function Candles({ candles, scale }: { candles: CandleBar[]; scale: PriceScale }) {
  if (candles.length === 0) return null;
  const innerW = W - PAD_L - PAD_R;
  // Reserve PROJ_FRAC of the inner width for forward projections;
  // candles render into the remaining (1 - PROJ_FRAC) so projected
  // anchors have somewhere to live without being clamped.
  const candleAreaW = innerW * (1 - PROJ_FRAC);
  const step = candleAreaW / candles.length;
  const bodyW = Math.max(1, step * 0.7);
  return (
    <g>
      {candles.map((c, i) => {
        const x = PAD_L + i * step + step / 2;
        const o = Number(c.open);
        const cl = Number(c.close);
        const hi = Number(c.high);
        const lo = Number(c.low);
        const up = cl >= o;
        const color = up ? "rgb(52 211 153)" : "rgb(248 113 113)";
        const yHi = scale.toY(hi);
        const yLo = scale.toY(lo);
        const yOpen = scale.toY(o);
        const yClose = scale.toY(cl);
        const bodyTop = Math.min(yOpen, yClose);
        const bodyH = Math.max(1, Math.abs(yOpen - yClose));
        return (
          <g key={c.open_time}>
            <line x1={x} x2={x} y1={yHi} y2={yLo} stroke={color} strokeWidth={1} />
            <rect
              x={x - bodyW / 2}
              y={bodyTop}
              width={bodyW}
              height={bodyH}
              fill={color}
              fillOpacity={0.6}
              stroke={color}
            />
          </g>
        );
      })}
    </g>
  );
}

// Family → palette. Adding a new family is one entry here, no scattered
// switch arms in the rendering code (CLAUDE.md #1 spirit, on the FE).
const FAMILY_COLORS: Record<string, string> = {
  elliott: "rgb(125 211 252)",     // sky-300
  harmonic: "rgb(244 114 182)",    // pink-400
  classical: "rgb(250 204 21)",    // yellow-400
  wyckoff: "rgb(167 139 250)",     // violet-400
  range: "rgb(94 234 212)",        // teal-300
  tbm: "rgb(251 146 60)",          // orange-400 — reversal/setup family
  custom: "rgb(212 212 216)",      // zinc-300
};

const STATE_DASH: Record<string, string> = {
  forming: "4 3",
  confirmed: "",
  completed: "",
  invalidated: "1 4",
};

function familyColor(family: string): string {
  return FAMILY_COLORS[family] ?? FAMILY_COLORS.custom;
}

function Detections({
  detections,
  candles,
  scale,
  hovered,
  onHover,
}: {
  detections: DetectionOverlay[];
  candles: CandleBar[];
  scale: PriceScale;
  hovered: string | null;
  onHover: (id: string | null) => void;
}) {
  if (detections.length === 0 || candles.length === 0) return null;
  const innerW = W - PAD_L - PAD_R;
  const candleAreaW = innerW * (1 - PROJ_FRAC);
  const projAreaW = innerW * PROJ_FRAC;
  const step = candleAreaW / candles.length;
  const tStart = new Date(candles[0].open_time).getTime();
  const tEnd = new Date(candles[candles.length - 1].open_time).getTime();
  const span = tEnd - tStart || 1;
  // Average bar interval — reused for projecting times in the right-
  // side projection zone.
  const barMs = candles.length >= 2 ? span / (candles.length - 1) : 60_000;

  // Time → x. Times inside the candle window land on the regular grid;
  // times beyond the last candle slide into the right-side projection
  // zone, scaled by bar interval so projections stay visually anchored
  // to the timeline rather than clamped to a single column.
  const xFor = (iso: string): number => {
    const t = new Date(iso).getTime();
    if (t <= tEnd) {
      const clamped = Math.max(tStart, t);
      const i = ((clamped - tStart) / span) * (candles.length - 1);
      return PAD_L + i * step + step / 2;
    }
    const offset = (t - tEnd) / barMs; // bars past the last candle
    const candleEdge = PAD_L + candleAreaW;
    // Cap at the right padding so absurdly far projections don't
    // overflow the chart frame.
    const dx = Math.min(projAreaW, offset * step);
    return candleEdge + dx;
  };

  return (
    <g>
      {detections.map((d) => {
        const color = familyColor(d.family);
        const dash = STATE_DASH[d.state] ?? "";
        const conf = Number(d.confidence) || 0;
        const baseOpacity = 0.35 + 0.55 * Math.min(1, Math.max(0, conf));
        const isHover = hovered === d.id;
        const opacity = isHover ? 1 : baseOpacity;
        const strokeW = isHover ? 2.5 : 1.5;

        const points: Array<{ x: number; y: number; label: string | null }> =
          d.anchors.length > 0
            ? d.anchors.map((a) => ({
                x: xFor(a.time),
                y: scale.toY(Number(a.price)),
                label: a.label,
              }))
            : [
                {
                  x: xFor(d.anchor_time),
                  y: scale.toY(Number(d.anchor_price)),
                  label: null,
                },
              ];

        const polyPoints = points.map((p) => `${p.x},${p.y}`).join(" ");
        const last = points[points.length - 1];
        const invalY = scale.toY(Number(d.invalidation_price));

        return (
          <g
            key={d.id}
            onMouseEnter={() => onHover(d.id)}
            onMouseLeave={() => onHover(null)}
            style={{ cursor: "pointer" }}
          >
            {/* invalidation (stop) line — only on hover so we don't
                clutter the chart with horizontal stripes for every
                detection. Spans from the last anchor a short way to
                the right (~15% of chart width) instead of the whole
                axis. */}
            {isHover && Number.isFinite(invalY) && points.length > 0 && (
              <line
                x1={last.x}
                x2={Math.min(W - PAD_R, last.x + (W - PAD_L - PAD_R) * 0.15)}
                y1={invalY}
                y2={invalY}
                stroke={color}
                strokeOpacity={0.7}
                strokeWidth={1}
                strokeDasharray="2 4"
              />
            )}

            {/* anchor polyline (the geometry of the pattern) */}
            {points.length >= 2 && (
              <polyline
                points={polyPoints}
                fill="none"
                stroke={color}
                strokeOpacity={opacity}
                strokeWidth={strokeW}
                strokeDasharray={dash}
                strokeLinejoin="round"
              />
            )}

            {/* Faz 7.6 / A3 — sub-wave decomposition: thinner +
                fainter polyline per realized segment. */}
            {(d.sub_wave_anchors ?? []).map((seg, si) => {
              if (seg.length < 2) return null;
              const segPts = seg
                .map((a) => `${xFor(a.time)},${scale.toY(Number(a.price))}`)
                .join(" ");
              return (
                <g key={`sw-${si}`}>
                  <polyline
                    points={segPts}
                    fill="none"
                    stroke={SUBWAVE_COLOR}
                    strokeOpacity={opacity * 0.7}
                    strokeWidth={strokeW * 0.7}
                    strokeLinejoin="round"
                    strokeDasharray="2 2"
                  />
                  {seg.map((a, i) => (
                    <circle
                      key={`sw-${si}-${i}`}
                      cx={xFor(a.time)}
                      cy={scale.toY(Number(a.price))}
                      r={1.6}
                      fill={SUBWAVE_COLOR}
                      fillOpacity={opacity * 0.7}
                    />
                  ))}
                </g>
              );
            })}

            {/* Faz 7.6 / A2 — forward projection: dashed continuation
                from the last realized anchor through projected pivots. */}
            {(d.projected_anchors ?? []).length > 0 && points.length > 0 && (() => {
              const proj = (d.projected_anchors ?? []).map((a) => ({
                x: xFor(a.time),
                y: scale.toY(Number(a.price)),
                label: a.label,
              }));
              const projPoly = [last, ...proj]
                .map((p) => `${p.x},${p.y}`)
                .join(" ");
              return (
                <g>
                  <polyline
                    points={projPoly}
                    fill="none"
                    stroke={color}
                    strokeOpacity={opacity * 0.8}
                    strokeWidth={strokeW}
                    strokeDasharray="4 3"
                    strokeLinejoin="round"
                  />
                  {proj.map((p, i) => (
                    <g key={`pj-${i}`}>
                      <circle
                        cx={p.x}
                        cy={p.y}
                        r={isHover ? 3 : 2.5}
                        fill="none"
                        stroke={color}
                        strokeOpacity={opacity * 0.8}
                        strokeWidth={1}
                      />
                      {p.label && (
                        <text
                          x={p.x + 4}
                          y={p.y - 4}
                          fontSize={9}
                          fill={color}
                          fillOpacity={opacity * 0.8}
                          fontFamily="ui-monospace, monospace"
                        >
                          {p.label}
                        </text>
                      )}
                    </g>
                  ))}
                </g>
              );
            })()}

            {/* per-pivot dots + optional labels */}
            {points.map((p, i) => (
              <g key={i}>
                <circle
                  cx={p.x}
                  cy={p.y}
                  r={isHover ? 4 : 3}
                  fill={color}
                  fillOpacity={opacity}
                  stroke="rgb(24 24 27)"
                  strokeWidth={1}
                />
                {p.label && (
                  <text
                    x={p.x + 4}
                    y={p.y - 4}
                    fontSize={9}
                    fill={color}
                    fillOpacity={opacity}
                    fontFamily="ui-monospace, monospace"
                  >
                    {p.label}
                  </text>
                )}
              </g>
            ))}

            {/* pattern label at the last anchor */}
            <text
              x={last.x + 6}
              y={last.y - 8}
              fontSize={10}
              fill={color}
              fillOpacity={Math.min(1, opacity + 0.15)}
              fontFamily="ui-monospace, monospace"
            >
              {d.subkind}
              {conf > 0 && ` · ${(conf * 100).toFixed(0)}%`}
            </text>
          </g>
        );
      })}
    </g>
  );
}

// Percent-reversal zigzag over the visible candles. Walks high/low,
// flips direction when price retraces by `pct` from the running
// extreme. Pure client-side — no backend dependency, so the toggle
// is instant.
function computeZigzag(
  candles: CandleBar[],
  pct: number,
): Array<{ idx: number; price: number; kind: "H" | "L" }> {
  if (candles.length < 2) return [];
  const pts: Array<{ idx: number; price: number; kind: "H" | "L" }> = [];
  let dir: "up" | "down" | null = null;
  let extIdx = 0;
  let extPrice = Number(candles[0].close);
  for (let i = 1; i < candles.length; i++) {
    const hi = Number(candles[i].high);
    const lo = Number(candles[i].low);
    if (dir === null) {
      if (hi >= extPrice * (1 + pct)) {
        pts.push({ idx: extIdx, price: extPrice, kind: "L" });
        dir = "up";
        extIdx = i;
        extPrice = hi;
      } else if (lo <= extPrice * (1 - pct)) {
        pts.push({ idx: extIdx, price: extPrice, kind: "H" });
        dir = "down";
        extIdx = i;
        extPrice = lo;
      }
    } else if (dir === "up") {
      if (hi >= extPrice) {
        extIdx = i;
        extPrice = hi;
      } else if (lo <= extPrice * (1 - pct)) {
        pts.push({ idx: extIdx, price: extPrice, kind: "H" });
        dir = "down";
        extIdx = i;
        extPrice = lo;
      }
    } else {
      if (lo <= extPrice) {
        extIdx = i;
        extPrice = lo;
      } else if (hi >= extPrice * (1 + pct)) {
        pts.push({ idx: extIdx, price: extPrice, kind: "L" });
        dir = "up";
        extIdx = i;
        extPrice = hi;
      }
    }
  }
  pts.push({ idx: extIdx, price: extPrice, kind: dir === "up" ? "H" : "L" });
  return pts;
}

function Zigzag({ candles, scale }: { candles: CandleBar[]; scale: PriceScale }) {
  if (candles.length === 0) return null;
  const pts = computeZigzag(candles, ZIGZAG_PCT);
  if (pts.length < 2) return null;
  const innerW = W - PAD_L - PAD_R;
  const candleAreaW = innerW * (1 - PROJ_FRAC);
  const step = candleAreaW / candles.length;
  const xFor = (i: number) => PAD_L + i * step + step / 2;
  const poly = pts.map((p) => `${xFor(p.idx)},${scale.toY(p.price)}`).join(" ");
  return (
    <g>
      <polyline
        points={poly}
        fill="none"
        stroke="rgb(250 204 21)"
        strokeOpacity={0.85}
        strokeWidth={1.5}
        strokeLinejoin="round"
      />
      {pts.map((p, i) => (
        <circle
          key={i}
          cx={xFor(p.idx)}
          cy={scale.toY(p.price)}
          r={2.5}
          fill="rgb(250 204 21)"
        />
      ))}
    </g>
  );
}

// Family toggle chips. Each chip is a button that flips that family's
// visibility on/off. Lives next to the symbol/timeframe controls so the
// operator can dim noisy families without scrolling. State is owned by
// the parent so the SVG layer can filter detections accordingly.
function FamilyToggles({
  enabled,
  onToggle,
}: {
  enabled: Record<string, boolean>;
  onToggle: (family: string) => void;
}) {
  const entries = Object.entries(FAMILY_COLORS).filter(([k]) => k !== "custom");
  return (
    <div className="flex flex-wrap items-center gap-2">
      {entries.map(([family, color]) => {
        const on = enabled[family] !== false;
        return (
          <button
            key={family}
            type="button"
            onClick={() => onToggle(family)}
            className="flex items-center gap-1.5 rounded border px-2 py-1 text-xs uppercase tracking-wide transition"
            style={{
              borderColor: on ? color : "rgb(63 63 70)",
              background: on ? `${color}22` : "rgb(24 24 27)",
              color: on ? color : "rgb(113 113 122)",
            }}
            title={on ? `${family}: görünür` : `${family}: gizli`}
          >
            <span
              className="inline-block h-2 w-3 rounded-sm"
              style={{ background: on ? color : "rgb(63 63 70)" }}
            />
            {family}
          </button>
        );
      })}
    </div>
  );
}

function StateLegend() {
  return (
    <div className="flex items-center gap-3 text-xs text-zinc-500">
      <div className="flex items-center gap-1.5">
        <span className="inline-block h-0 w-4 border-t border-dashed border-zinc-500" />
        <span>forming</span>
      </div>
      <div className="flex items-center gap-1.5">
        <span className="inline-block h-0 w-4 border-t border-zinc-500" />
        <span>confirmed</span>
      </div>
    </div>
  );
}

function Axis({ scale }: { scale: PriceScale }) {
  // Five ticks: min, +25%, +50%, +75%, max.
  const ticks = [0, 0.25, 0.5, 0.75, 1].map((f) => scale.min + (scale.max - scale.min) * f);
  return (
    <g>
      {ticks.map((t) => {
        const y = scale.toY(t);
        return (
          <g key={t}>
            <line x1={PAD_L} x2={W - PAD_R} y1={y} y2={y} stroke="rgb(39 39 42)" strokeDasharray="2 4" />
            <text x={PAD_L - 6} y={y + 3} fontSize={10} textAnchor="end" fill="rgb(113 113 122)">
              {t.toFixed(2)}
            </text>
          </g>
        );
      })}
    </g>
  );
}

// Bottom time axis. Picks 6 evenly spaced candles, formats as
// "MM-DD HH:mm" so an operator can correlate with logs at a glance.
function TimeAxis({ candles }: { candles: CandleBar[] }) {
  if (candles.length === 0) return null;
  const innerW = W - PAD_L - PAD_R;
  const candleAreaW = innerW * (1 - PROJ_FRAC);
  const step = candleAreaW / candles.length;
  const ticks = 6;
  const idxs = Array.from({ length: ticks }, (_, i) =>
    Math.floor((i / (ticks - 1)) * (candles.length - 1)),
  );
  const fmt = (iso: string) => {
    const d = new Date(iso);
    const pad = (n: number) => n.toString().padStart(2, "0");
    return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(
      d.getMinutes(),
    )}`;
  };
  return (
    <g>
      <line
        x1={PAD_L}
        x2={W - PAD_R}
        y1={H - PAD_B + 2}
        y2={H - PAD_B + 2}
        stroke="rgb(39 39 42)"
      />
      {idxs.map((i) => {
        const x = PAD_L + i * step + step / 2;
        return (
          <g key={i}>
            <line
              x1={x}
              x2={x}
              y1={H - PAD_B + 2}
              y2={H - PAD_B + 6}
              stroke="rgb(82 82 91)"
            />
            <text
              x={x}
              y={H - PAD_B + 18}
              fontSize={10}
              textAnchor="middle"
              fill="rgb(113 113 122)"
              fontFamily="ui-monospace, monospace"
            >
              {fmt(candles[i].open_time)}
            </text>
          </g>
        );
      })}
    </g>
  );
}

interface ChartForm {
  venue: string;
  symbol: string;
  timeframe: string;
}

export function Chart() {
  const [form, setForm] = useState<ChartForm>(DEFAULTS);
  // Debounced form copy used as the actual query key — typing in the
  // inputs doesn't fire a request on every keystroke.
  const [debounced, setDebounced] = useState<ChartForm>(DEFAULTS);
  // Pan-left history pages, oldest first. Each entry is a window of
  // candles + detections fetched with `?before=`.
  const [olderPages, setOlderPages] = useState<ChartWorkspace[]>([]);
  const [hovered, setHovered] = useState<string | null>(null);
  // Viewport into the merged candle array. `viewOffsetFromEnd` = how
  // many newest candles are hidden off the right edge (0 = anchored
  // to live tail). `viewSize` = number of candles currently rendered.
  const [viewOffsetFromEnd, setViewOffsetFromEnd] = useState(0);
  const [viewSize, setViewSize] = useState(DEFAULT_VIEW_SIZE);
  const [isDragging, setIsDragging] = useState(false);
  const [hoverContainer, setHoverContainer] = useState(false);
  const [showZigzag, setShowZigzag] = useState(false);
  const [showVolume, setShowVolume] = useState(true);
  // Per-family visibility. Defaults to all-on; clicking a chip in the
  // top toolbar flips that family. Sparse map (missing key = visible)
  // so adding new families needs no migration here.
  const [familyEnabled, setFamilyEnabled] = useState<Record<string, boolean>>({});
  const toggleFamily = (family: string) =>
    setFamilyEnabled((prev) => ({ ...prev, [family]: prev[family] === false }));
  const fetchingOlderRef = useRef(false);
  const dragRef = useRef<{
    startX: number;
    startOffset: number;
    pxPerCandle: number;
  } | null>(null);
  const svgRef = useRef<SVGSVGElement | null>(null);

  // Reset paging + viewport whenever the symbol/tf changes.
  useEffect(() => {
    setOlderPages([]);
    setViewOffsetFromEnd(0);
    setViewSize(DEFAULT_VIEW_SIZE);
  }, [debounced.venue, debounced.symbol, debounced.timeframe]);

  useEffect(() => {
    const t = setTimeout(() => setDebounced(form), 300);
    return () => clearTimeout(t);
  }, [form]);

  const query = useQuery({
    queryKey: ["v2", "chart", debounced],
    queryFn: () =>
      apiFetch<ChartWorkspace>(
        `/v2/chart/${debounced.venue}/${debounced.symbol}/${debounced.timeframe}?limit=${PAGE_SIZE}`,
      ),
    refetchInterval: 10_000,
  });

  // Merge any older pages in front of the live tail. Detections are
  // de-duplicated by id so the panning history doesn't double-count.
  const merged = useMemo<ChartWorkspace | undefined>(() => {
    if (!query.data) return undefined;
    if (olderPages.length === 0) return query.data;
    const seen = new Set<string>();
    const detections: DetectionOverlay[] = [];
    for (const page of [...olderPages, query.data]) {
      for (const d of page.detections) {
        if (seen.has(d.id)) continue;
        seen.add(d.id);
        detections.push(d);
      }
    }
    const candles: CandleBar[] = [];
    const seenT = new Set<string>();
    for (const page of [...olderPages, query.data]) {
      for (const c of page.candles) {
        if (seenT.has(c.open_time)) continue;
        seenT.add(c.open_time);
        candles.push(c);
      }
    }
    candles.sort((a, b) => a.open_time.localeCompare(b.open_time));
    return { ...query.data, candles, detections };
  }, [query.data, olderPages]);

  // Visible slice of the merged candle array based on viewport state.
  // We render only this slice; the price/time scales are derived from
  // it so zooming changes both axes the way TradingView does.
  const totalCandles = merged?.candles.length ?? 0;
  const effectiveViewSize = Math.min(viewSize, Math.max(MIN_VIEW_SIZE, totalCandles));
  const maxOffset = Math.max(0, totalCandles - effectiveViewSize);
  const clampedOffset = Math.min(viewOffsetFromEnd, maxOffset);
  const visibleEnd = totalCandles - clampedOffset;
  const visibleStart = Math.max(0, visibleEnd - effectiveViewSize);
  const visibleCandles = useMemo(
    () => (merged ? merged.candles.slice(visibleStart, visibleEnd) : []),
    [merged, visibleStart, visibleEnd],
  );
  const scale = useMemo(() => buildScale(visibleCandles), [visibleCandles]);
  const visibleDetections = useMemo(
    () =>
      merged
        ? merged.detections.filter((d) => familyEnabled[d.family] !== false)
        : [],
    [merged, familyEnabled],
  );

  // Auto-prefetch older history when the user pans close to the left
  // edge of the loaded buffer. Fires once per page load (guarded by
  // fetchingOlderRef) so spam-dragging doesn't queue up duplicates.
  const fetchOlder = async () => {
    if (fetchingOlderRef.current || !merged || merged.candles.length === 0) return;
    fetchingOlderRef.current = true;
    try {
      const oldest = merged.candles[0].open_time;
      const page = await apiFetch<ChartWorkspace>(
        `/v2/chart/${debounced.venue}/${debounced.symbol}/${debounced.timeframe}?limit=${PAGE_SIZE}&before=${encodeURIComponent(
          oldest,
        )}`,
      );
      if (page.candles.length > 0) {
        setOlderPages((prev) => [page, ...prev]);
      }
    } catch (err) {
      console.error("pan-left fetch failed", err);
    } finally {
      fetchingOlderRef.current = false;
    }
  };

  useEffect(() => {
    if (!merged) return;
    if (visibleStart <= PREFETCH_THRESHOLD) {
      fetchOlder();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visibleStart, merged?.candles.length]);

  // ----- TradingView-style mouse pan -----
  // Drag updates viewOffsetFromEnd live as the mouse moves; pixels →
  // candles via the current effective view size. Right-drag = show
  // older history (offset grows), left-drag = walk back toward live.
  const innerW = W - PAD_L - PAD_R;
  const onMouseDown = (e: React.MouseEvent) => {
    if (effectiveViewSize === 0) return;
    dragRef.current = {
      startX: e.clientX,
      startOffset: clampedOffset,
      pxPerCandle: innerW / effectiveViewSize,
    };
    setIsDragging(true);
  };
  const onMouseMove = (e: React.MouseEvent) => {
    const drag = dragRef.current;
    if (!drag) return;
    const dx = e.clientX - drag.startX;
    const candleDelta = Math.round(dx / drag.pxPerCandle);
    const next = Math.max(0, Math.min(maxOffset, drag.startOffset + candleDelta));
    setViewOffsetFromEnd(next);
  };
  const endDrag = () => {
    if (!dragRef.current) return;
    dragRef.current = null;
    setIsDragging(false);
  };

  // ----- Zoom controls -----
  const zoomBy = (factor: number) => {
    setViewSize((prev) => {
      const next = Math.round(prev * factor);
      return Math.max(MIN_VIEW_SIZE, Math.min(MAX_VIEW_SIZE, next));
    });
  };
  const onWheel = (e: React.WheelEvent) => {
    e.preventDefault();
    zoomBy(e.deltaY > 0 ? ZOOM_FACTOR : 1 / ZOOM_FACTOR);
  };
  const resetView = () => {
    setViewSize(DEFAULT_VIEW_SIZE);
    setViewOffsetFromEnd(0);
  };

  const showControls = hoverContainer && !isDragging;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-end gap-3 rounded-lg border border-zinc-800 bg-zinc-900/60 p-4 text-sm">
        <label className="flex flex-col gap-1">
          <span className="text-xs uppercase text-zinc-500">Venue</span>
          <input
            value={form.venue}
            onChange={(e) => setForm({ ...form, venue: e.target.value })}
            className="w-32 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-zinc-100"
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-xs uppercase text-zinc-500">Symbol</span>
          <input
            value={form.symbol}
            onChange={(e) => setForm({ ...form, symbol: e.target.value })}
            className="w-32 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 font-mono text-zinc-100"
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-xs uppercase text-zinc-500">Timeframe</span>
          <input
            value={form.timeframe}
            onChange={(e) => setForm({ ...form, timeframe: e.target.value })}
            className="w-20 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 font-mono text-zinc-100"
          />
        </label>
        <div className="ml-auto flex flex-col items-end gap-2">
          <FamilyToggles enabled={familyEnabled} onToggle={toggleFamily} />
          <div className="text-xs text-zinc-500">
            Otomatik yükleme · sürükle = pan · scroll/+/− = zoom
          </div>
        </div>
      </div>

      {query.isLoading && !merged && (
        <div className="text-sm text-zinc-400">Loading chart…</div>
      )}
      {query.isError && (
        <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
          Failed: {(query.error as Error).message}
        </div>
      )}
      {merged && (
        <>
          <div className="flex items-center justify-between text-xs text-zinc-500">
            <span>
              {visibleCandles.length} / {merged.candles.length} candles ·{" "}
              {visibleDetections.length}/{merged.detections.length} detections · {merged.positions.length} open positions ·{" "}
              {merged.open_orders.length} working orders
            </span>
            <StateLegend />
          </div>
          <div
            className="relative"
            onMouseEnter={() => setHoverContainer(true)}
            onMouseLeave={() => {
              setHoverContainer(false);
              endDrag();
            }}
          >
            <svg
              ref={svgRef}
              viewBox={`0 0 ${W} ${H}`}
              className="w-full rounded border border-zinc-800 bg-zinc-950 select-none"
              onMouseDown={onMouseDown}
              onMouseMove={onMouseMove}
              onMouseUp={endDrag}
              onWheel={onWheel}
              style={{ cursor: isDragging ? "grabbing" : "grab" }}
            >
              <Axis scale={scale} />
              {showVolume && <Volume candles={visibleCandles} />}
              <Candles candles={visibleCandles} scale={scale} />
              {showZigzag && <Zigzag candles={visibleCandles} scale={scale} />}
              <Detections
                detections={visibleDetections}
                candles={visibleCandles}
                scale={scale}
                hovered={hovered}
                onHover={setHovered}
              />
              <TimeAxis candles={visibleCandles} />
            </svg>
            {/* Floating zoom controls — TradingView-style. Hidden by
                default, fade in on container hover, hide while
                dragging so they don't fight the pan gesture. */}
            <div
              className={`pointer-events-none absolute right-4 top-4 flex flex-col gap-1 transition-opacity duration-150 ${
                showControls ? "opacity-100" : "opacity-0"
              }`}
            >
              <button
                type="button"
                onClick={() => zoomBy(1 / ZOOM_FACTOR)}
                title="Zoom in"
                className="pointer-events-auto h-7 w-7 rounded border border-zinc-700 bg-zinc-900/90 text-zinc-200 hover:bg-zinc-800"
              >
                +
              </button>
              <button
                type="button"
                onClick={() => zoomBy(ZOOM_FACTOR)}
                title="Zoom out"
                className="pointer-events-auto h-7 w-7 rounded border border-zinc-700 bg-zinc-900/90 text-zinc-200 hover:bg-zinc-800"
              >
                −
              </button>
              <button
                type="button"
                onClick={() => setShowVolume((v) => !v)}
                title={showVolume ? "Hide volume" : "Show volume"}
                className={`pointer-events-auto h-7 w-7 rounded border text-[10px] ${
                  showVolume
                    ? "border-emerald-500 bg-emerald-500/20 text-emerald-300"
                    : "border-zinc-700 bg-zinc-900/90 text-zinc-200 hover:bg-zinc-800"
                }`}
              >
                V
              </button>
              <button
                type="button"
                onClick={() => setShowZigzag((v) => !v)}
                title={showZigzag ? "Hide zigzag" : "Show zigzag"}
                className={`pointer-events-auto h-7 w-7 rounded border text-[10px] ${
                  showZigzag
                    ? "border-yellow-500 bg-yellow-500/20 text-yellow-300"
                    : "border-zinc-700 bg-zinc-900/90 text-zinc-200 hover:bg-zinc-800"
                }`}
              >
                Z
              </button>
              <button
                type="button"
                onClick={resetView}
                title="Reset"
                className="pointer-events-auto h-7 w-7 rounded border border-zinc-700 bg-zinc-900/90 text-[10px] text-zinc-200 hover:bg-zinc-800"
              >
                ⟳
              </button>
            </div>
          </div>

          {merged.detections.length > 0 && (
            <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-4">
              <div className="mb-2 text-xs uppercase tracking-wide text-zinc-500">
                Detections
              </div>
              <table className="w-full text-sm">
                <thead className="text-xs uppercase text-zinc-500">
                  <tr>
                    <th className="px-2 py-1 text-left">Kind</th>
                    <th className="px-2 py-1 text-left">State</th>
                    <th className="px-2 py-1 text-left">When</th>
                    <th className="px-2 py-1 text-right">Price</th>
                    <th className="px-2 py-1 text-right">Stop</th>
                    <th className="px-2 py-1 text-right">Conf</th>
                  </tr>
                </thead>
                <tbody className="font-mono text-zinc-100">
                  {merged.detections.map((d) => (
                    <tr
                      key={d.id}
                      className={`border-t border-zinc-800/60 ${
                        hovered === d.id ? "bg-zinc-800/60" : ""
                      }`}
                      onMouseEnter={() => setHovered(d.id)}
                      onMouseLeave={() => setHovered(null)}
                    >
                      <td className="px-2 py-1">{d.kind}</td>
                      <td className="px-2 py-1 text-zinc-400">{d.state}</td>
                      <td className="px-2 py-1 text-zinc-400">{d.anchor_time}</td>
                      <td className="px-2 py-1 text-right">{d.anchor_price}</td>
                      <td className="px-2 py-1 text-right text-zinc-500">
                        {d.invalidation_price}
                      </td>
                      <td className="px-2 py-1 text-right">{d.confidence}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </div>
  );
}
