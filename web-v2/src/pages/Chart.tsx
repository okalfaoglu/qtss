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

interface WyckoffOverlayData {
  id: string;
  schematic: string;
  phase: string;
  confidence: number | null;
  range: { top: number | null; bottom: number | null };
  creek: number | null;
  ice: number | null;
  slope_deg: number | null;
  events: Array<{ event: string; bar_index: number; price: number; score: number }>;
  started_at: string;
}

const WYCKOFF_PHASE_COLORS: Record<string, string> = {
  A: "rgb(239 68 68)",   // red
  B: "rgb(249 115 22)",  // orange
  C: "rgb(234 179 8)",   // yellow
  D: "rgb(34 197 94)",   // green
  E: "rgb(59 130 246)",  // blue
};

interface PriceScale {
  min: number;
  max: number;
  toY: (price: number) => number;
}

function buildScale(candles: CandleBar[], yOffset = 0, yZoom = 1): PriceScale {
  if (candles.length === 0) {
    return { min: 0, max: 1, toY: () => H / 2 };
  }
  let rawMin = Infinity;
  let rawMax = -Infinity;
  for (const c of candles) {
    const lo = Number(c.low);
    const hi = Number(c.high);
    if (lo < rawMin) rawMin = lo;
    if (hi > rawMax) rawMax = hi;
  }
  const rawSpan = rawMax - rawMin || 1;
  // Apply vertical zoom: shrink/expand range around center
  const center = (rawMin + rawMax) / 2;
  const halfSpan = (rawSpan / 2) / yZoom;
  // Apply vertical offset (in price units)
  const min = center - halfSpan + yOffset;
  const max = center + halfSpan + yOffset;
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

// Sub-kind specific colors for range sub-detectors.
// Each sub-detector gets a distinct color for visual clarity.
const RANGE_SUBKIND_COLORS: Record<string, string> = {
  bullish_fvg:         "rgb(52 211 153)",   // emerald-400
  bearish_fvg:         "rgb(248 113 113)",  // red-400
  bullish_ob:          "rgb(96 165 250)",   // blue-400
  bearish_ob:          "rgb(251 146 60)",   // orange-400
  liquidity_pool_high: "rgb(250 204 21)",   // yellow-400
  liquidity_pool_low:  "rgb(250 204 21)",   // yellow-400
  equal_highs:         "rgb(192 132 252)",  // purple-400
  equal_lows:          "rgb(192 132 252)",  // purple-400
};

// Sub-kinds that should render as a zone box (rect) instead of polyline.
const ZONE_BOX_SUBKINDS = new Set([
  "bullish_fvg", "bearish_fvg",
  "bullish_ob", "bearish_ob",
  "liquidity_pool_high", "liquidity_pool_low",
  "equal_highs", "equal_lows",
]);

// Labels for human-readable display.
const RANGE_SUBKIND_LABELS: Record<string, string> = {
  bullish_fvg:         "FVG ▲",
  bearish_fvg:         "FVG ▼",
  bullish_ob:          "OB ▲",
  bearish_ob:          "OB ▼",
  liquidity_pool_high: "LIQ ═",
  liquidity_pool_low:  "LIQ ═",
  equal_highs:         "EQH ═",
  equal_lows:          "EQL ═",
};

const STATE_DASH: Record<string, string> = {
  forming: "4 3",
  confirmed: "",
  completed: "",
  invalidated: "1 4",
};

function familyColor(family: string, subkind?: string): string {
  if (family === "range" && subkind && RANGE_SUBKIND_COLORS[subkind]) {
    return RANGE_SUBKIND_COLORS[subkind];
  }
  return FAMILY_COLORS[family] ?? FAMILY_COLORS.custom;
}

function Detections({
  detections,
  candles,
  scale,
  hovered,
  onHover,
  familyModes,
}: {
  detections: DetectionOverlay[];
  candles: CandleBar[];
  scale: PriceScale;
  hovered: string | null;
  onHover: (id: string | null) => void;
  familyModes: Record<string, FamilyMode>;
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
        const color = familyColor(d.family, d.subkind);
        const dash = STATE_DASH[d.state] ?? "";
        const conf = Number(d.confidence) || 0;
        const baseOpacity = 0.35 + 0.55 * Math.min(1, Math.max(0, conf));
        const isHover = hovered === d.id;
        const opacity = isHover ? 1 : baseOpacity;
        const strokeW = isHover ? 2.5 : 1.5;
        const isZoneBox = ZONE_BOX_SUBKINDS.has(d.subkind);

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

        // Zone box geometry: two anchors define top/bottom of the zone.
        // Box extends left by ~30 candles and right to the last anchor x.
        const zoneBoxWidth = step * 25;
        const zoneLabel = RANGE_SUBKIND_LABELS[d.subkind] ?? d.subkind;

        return (
          <g
            key={d.id}
            onMouseEnter={() => onHover(d.id)}
            onMouseLeave={() => onHover(null)}
            style={{ cursor: "pointer" }}
          >
            {/* ── Zone box rendering for FVG / OB / Liquidity / Equal ── */}
            {isZoneBox && points.length >= 2 && (() => {
              const yTop = Math.min(points[0].y, points[1].y);
              const yBot = Math.max(points[0].y, points[1].y);
              const boxH = Math.max(yBot - yTop, 2);
              const boxX = last.x - zoneBoxWidth;
              return (
                <g>
                  {/* Semi-transparent zone fill */}
                  <rect
                    x={boxX}
                    y={yTop}
                    width={zoneBoxWidth}
                    height={boxH}
                    fill={color}
                    fillOpacity={isHover ? 0.25 : 0.12}
                    rx={2}
                  />
                  {/* Zone border */}
                  <rect
                    x={boxX}
                    y={yTop}
                    width={zoneBoxWidth}
                    height={boxH}
                    fill="none"
                    stroke={color}
                    strokeOpacity={opacity * 0.8}
                    strokeWidth={isHover ? 1.5 : 0.8}
                    strokeDasharray={d.subkind.includes("fvg") ? "" : "3 2"}
                    rx={2}
                  />
                  {/* Top edge line (solid for emphasis) */}
                  <line
                    x1={boxX} x2={boxX + zoneBoxWidth}
                    y1={yTop} y2={yTop}
                    stroke={color}
                    strokeOpacity={opacity}
                    strokeWidth={isHover ? 1.5 : 1}
                  />
                  {/* Bottom edge line */}
                  <line
                    x1={boxX} x2={boxX + zoneBoxWidth}
                    y1={yBot} y2={yBot}
                    stroke={color}
                    strokeOpacity={opacity}
                    strokeWidth={isHover ? 1.5 : 1}
                  />
                  {/* Direction arrow inside box */}
                  <text
                    x={boxX + 4}
                    y={yTop + boxH / 2 + 4}
                    fontSize={11}
                    fill={color}
                    fillOpacity={isHover ? 0.9 : 0.6}
                    fontFamily="ui-monospace, monospace"
                    fontWeight="bold"
                  >
                    {zoneLabel}
                  </text>
                  {/* Confidence badge on hover */}
                  {isHover && conf > 0 && (
                    <text
                      x={boxX + zoneBoxWidth - 4}
                      y={yTop - 4}
                      fontSize={9}
                      fill={color}
                      fillOpacity={0.9}
                      fontFamily="ui-monospace, monospace"
                      textAnchor="end"
                    >
                      {(conf * 100).toFixed(0)}%
                    </text>
                  )}
                </g>
              );
            })()}

            {/* ── Entry / TP / SL lines (detail mode or hover) ── */}
            {(() => {
              const showDetail = (familyModes[d.family] ?? "on") === "detail" || isHover;
              if (!showDetail || points.length === 0) return null;

              const lineX1 = last.x;
              const lineX2 = Math.min(W - PAD_R, last.x + (W - PAD_L - PAD_R) * 0.25);
              const inv = Number(d.invalidation_price);

              // Compute measured-move targets from anchors.
              const anchors = d.anchors;
              let tp1: number | null = null;
              let tp2: number | null = null;
              let entryPrice: number | null = null;

              if (d.subkind.includes("double_top") || d.subkind.includes("double_bottom")) {
                if (anchors.length >= 3) {
                  const extreme = Number(anchors[0].price);
                  const neck = Number(anchors[1].price);
                  const height = Math.abs(extreme - neck);
                  const dir = d.subkind.includes("bull") ? 1 : -1;
                  entryPrice = neck;
                  tp1 = neck + dir * height;
                  tp2 = neck + dir * height * 1.618;
                }
              } else if (d.subkind.includes("head_and_shoulders")) {
                if (anchors.length >= 5) {
                  const head = Number(anchors[2].price);
                  const n1 = Number(anchors[1].price);
                  const n2 = Number(anchors[3].price);
                  const neckline = (n1 + n2) / 2;
                  const height = Math.abs(head - neckline);
                  const dir = d.subkind.includes("bull") ? 1 : -1;
                  entryPrice = neckline;
                  tp1 = neckline + dir * height;
                  tp2 = neckline + dir * height * 1.618;
                }
              } else if (d.family === "harmonic" && anchors.length >= 5) {
                const aP = Number(anchors[1].price);
                const dP = Number(anchors[4].price);
                const adRange = Math.abs(aP - dP);
                const dir = d.subkind.includes("bull") ? 1 : -1;
                entryPrice = dP;
                tp1 = dP + dir * adRange * 0.382;
                tp2 = dP + dir * adRange * 0.618;
              } else if (d.subkind.includes("impulse") && anchors.length >= 6) {
                const p0 = Number(anchors[0].price);
                const p1 = Number(anchors[1].price);
                const p4 = Number(anchors[4].price);
                const w1h = Math.abs(p1 - p0);
                const dir = d.subkind.includes("bull") ? 1 : -1;
                entryPrice = p4;
                tp1 = p4 + dir * w1h;
                tp2 = p4 + dir * w1h * 1.618;
              }

              // Label rendering helper
              const labelAt = (y: number, text: string, col: string) => (
                <text
                  x={lineX2 + 3}
                  y={y + 3}
                  fontSize={9}
                  fill={col}
                  fillOpacity={0.9}
                >
                  {text}
                </text>
              );

              return (
                <g>
                  {/* SL (invalidation) — red dashed */}
                  {Number.isFinite(inv) && (
                    <>
                      <line x1={lineX1} x2={lineX2} y1={scale.toY(inv)} y2={scale.toY(inv)}
                        stroke="rgb(239 68 68)" strokeOpacity={0.8} strokeWidth={1.2} strokeDasharray="4 3" />
                      {labelAt(scale.toY(inv), `SL ${inv.toFixed(2)}`, "rgb(239 68 68)")}
                    </>
                  )}
                  {/* Entry — white solid */}
                  {entryPrice && Number.isFinite(entryPrice) && (
                    <>
                      <line x1={lineX1} x2={lineX2} y1={scale.toY(entryPrice)} y2={scale.toY(entryPrice)}
                        stroke="rgb(212 212 216)" strokeOpacity={0.8} strokeWidth={1.2} strokeDasharray="2 2" />
                      {labelAt(scale.toY(entryPrice), `Entry ${entryPrice.toFixed(2)}`, "rgb(212 212 216)")}
                    </>
                  )}
                  {/* TP1 — green solid */}
                  {tp1 && Number.isFinite(tp1) && (
                    <>
                      <line x1={lineX1} x2={lineX2} y1={scale.toY(tp1)} y2={scale.toY(tp1)}
                        stroke="rgb(52 211 153)" strokeOpacity={0.8} strokeWidth={1.2} strokeDasharray="4 3" />
                      {labelAt(scale.toY(tp1), `TP1 ${tp1.toFixed(2)}`, "rgb(52 211 153)")}
                    </>
                  )}
                  {/* TP2 — green dashed fainter */}
                  {tp2 && Number.isFinite(tp2) && (
                    <>
                      <line x1={lineX1} x2={lineX2} y1={scale.toY(tp2)} y2={scale.toY(tp2)}
                        stroke="rgb(52 211 153)" strokeOpacity={0.5} strokeWidth={1} strokeDasharray="2 4" />
                      {labelAt(scale.toY(tp2), `TP2 ${tp2.toFixed(2)}`, "rgb(52 211 153)")}
                    </>
                  )}
                </g>
              );
            })()}

            {/* anchor polyline (the geometry of the pattern) — skip for zone boxes */}
            {!isZoneBox && points.length >= 2 && (
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

            {/* pattern label at the last anchor — zone boxes have their own label inside */}
            {!isZoneBox && (
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
            )}
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
type SwingLabel = "HH" | "LH" | "HL" | "LL" | null;
type ZigzagPoint = { idx: number; price: number; kind: "H" | "L"; dir: number; swing: SwingLabel };

function computeZigzag(
  candles: CandleBar[],
  pct: number,
): ZigzagPoint[] {
  if (candles.length < 2) return [];
  const raw: Array<{ idx: number; price: number; kind: "H" | "L" }> = [];
  let dir: "up" | "down" | null = null;
  let extIdx = 0;
  let extPrice = Number(candles[0].close);
  for (let i = 1; i < candles.length; i++) {
    const hi = Number(candles[i].high);
    const lo = Number(candles[i].low);
    if (dir === null) {
      if (hi >= extPrice * (1 + pct)) {
        raw.push({ idx: extIdx, price: extPrice, kind: "L" });
        dir = "up";
        extIdx = i;
        extPrice = hi;
      } else if (lo <= extPrice * (1 - pct)) {
        raw.push({ idx: extIdx, price: extPrice, kind: "H" });
        dir = "down";
        extIdx = i;
        extPrice = lo;
      }
    } else if (dir === "up") {
      if (hi >= extPrice) {
        extIdx = i;
        extPrice = hi;
      } else if (lo <= extPrice * (1 - pct)) {
        raw.push({ idx: extIdx, price: extPrice, kind: "H" });
        dir = "down";
        extIdx = i;
        extPrice = lo;
      }
    } else {
      if (lo <= extPrice) {
        extIdx = i;
        extPrice = lo;
      } else if (hi >= extPrice * (1 + pct)) {
        raw.push({ idx: extIdx, price: extPrice, kind: "L" });
        dir = "up";
        extIdx = i;
        extPrice = hi;
      }
    }
  }
  raw.push({ idx: extIdx, price: extPrice, kind: dir === "up" ? "H" : "L" });

  // Classify swing types: compare each pivot with previous same-kind pivot.
  // dir: +1 = uptrend confirmed (HL), -1 = downtrend confirmed (LH),
  //      +2 = strong uptrend (HH), -2 = strong downtrend (LL)
  let prevH: number | null = null;
  let prevL: number | null = null;
  const pts: ZigzagPoint[] = raw.map((p) => {
    let swing: SwingLabel = null;
    let d = 0;
    if (p.kind === "H") {
      if (prevH !== null) {
        swing = p.price >= prevH ? "HH" : "LH";
        d = swing === "HH" ? 2 : -1;
      }
      prevH = p.price;
    } else {
      if (prevL !== null) {
        swing = p.price >= prevL ? "HL" : "LL";
        d = swing === "HL" ? 1 : -2;
      }
      prevL = p.price;
    }
    return { ...p, dir: d, swing };
  });
  return pts;
}

// =========================================================================
// Wyckoff Structure Overlay (Faz 10)
// =========================================================================
// Renders: range box, creek/ice lines, event labels, phase badge.

function WyckoffStructureOverlay({
  data,
  scale,
}: {
  data: WyckoffOverlayData;
  scale: PriceScale;
}) {
  const innerW = W - PAD_L - PAD_R;
  const rangeTop = data.range.top;
  const rangeBot = data.range.bottom;
  if (rangeTop == null || rangeBot == null) return null;

  const yTop = scale.toY(rangeTop);
  const yBot = scale.toY(rangeBot);
  const boxH = yBot - yTop;
  if (boxH <= 0) return null;

  const phaseColor = WYCKOFF_PHASE_COLORS[data.phase] ?? "rgb(156 163 175)";
  const isAccum = data.schematic === "accumulation" || data.schematic === "reaccumulation";
  const boxFill = isAccum ? "rgba(34,197,94,0.06)" : "rgba(239,68,68,0.06)";
  const boxStroke = isAccum ? "rgba(34,197,94,0.3)" : "rgba(239,68,68,0.3)";

  return (
    <g className="wyckoff-overlay">
      {/* Range box */}
      <rect
        x={PAD_L}
        y={yTop}
        width={innerW}
        height={boxH}
        fill={boxFill}
        stroke={boxStroke}
        strokeWidth={1}
        strokeDasharray="6 3"
      />
      {/* Creek line */}
      {data.creek != null && (
        <line
          x1={PAD_L}
          y1={scale.toY(data.creek)}
          x2={PAD_L + innerW}
          y2={scale.toY(data.creek)}
          stroke="rgba(59,130,246,0.5)"
          strokeWidth={1}
          strokeDasharray="4 4"
        />
      )}
      {data.creek != null && (
        <text
          x={PAD_L + 4}
          y={scale.toY(data.creek) - 3}
          fill="rgba(59,130,246,0.7)"
          fontSize={9}
        >
          Creek
        </text>
      )}
      {/* Ice line */}
      {data.ice != null && (
        <line
          x1={PAD_L}
          y1={scale.toY(data.ice)}
          x2={PAD_L + innerW}
          y2={scale.toY(data.ice)}
          stroke="rgba(239,68,68,0.5)"
          strokeWidth={1}
          strokeDasharray="4 4"
        />
      )}
      {data.ice != null && (
        <text
          x={PAD_L + 4}
          y={scale.toY(data.ice) - 3}
          fill="rgba(239,68,68,0.7)"
          fontSize={9}
        >
          Ice
        </text>
      )}
      {/* Phase badge */}
      <rect
        x={PAD_L + innerW - 120}
        y={yTop + 4}
        width={116}
        height={18}
        rx={4}
        fill="rgba(0,0,0,0.7)"
        stroke={phaseColor}
        strokeWidth={1}
      />
      <text
        x={PAD_L + innerW - 116}
        y={yTop + 16}
        fill={phaseColor}
        fontSize={10}
        fontWeight="bold"
      >
        {data.schematic.toUpperCase()} · Phase {data.phase}
        {data.confidence != null ? ` · ${(data.confidence * 100).toFixed(0)}%` : ""}
      </text>
      {/* Event labels */}
      {data.events.map((ev, i) => {
        const yEv = scale.toY(ev.price);
        // Distribute labels horizontally across the range box
        const xFrac = data.events.length > 1 ? i / (data.events.length - 1) : 0.5;
        const xEv = PAD_L + innerW * 0.05 + innerW * 0.9 * xFrac;
        return (
          <g key={`${ev.event}-${i}`}>
            <circle cx={xEv} cy={yEv} r={3} fill={phaseColor} />
            <text
              x={xEv}
              y={yEv - 6}
              fill="rgb(229 231 235)"
              fontSize={8}
              textAnchor="middle"
              fontWeight="bold"
            >
              {ev.event}
            </text>
          </g>
        );
      })}
    </g>
  );
}

const SWING_COLORS: Record<string, string> = {
  HH: "rgb(34 197 94)",   // green — bullish continuation
  HL: "rgb(74 222 128)",   // light green — bullish base
  LH: "rgb(239 68 68)",    // red — bearish base
  LL: "rgb(248 113 113)",  // light red — bearish continuation
};

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
      {pts.map((p, i) => {
        const cx = xFor(p.idx);
        const cy = scale.toY(p.price);
        const col = p.swing ? (SWING_COLORS[p.swing] ?? "rgb(250 204 21)") : "rgb(250 204 21)";
        const isUp = p.kind === "H";
        return (
          <g key={i}>
            <circle cx={cx} cy={cy} r={2.5} fill={col} />
            {p.swing && (
              <text
                x={cx}
                y={isUp ? cy - 7 : cy + 13}
                textAnchor="middle"
                fill={col}
                fontSize={9}
                fontWeight={Math.abs(p.dir) === 2 ? 700 : 400}
                fontFamily="ui-monospace, monospace"
              >
                {p.swing}
              </text>
            )}
            {Math.abs(p.dir) === 2 && (
              <text
                x={cx}
                y={isUp ? cy - 17 : cy + 23}
                textAnchor="middle"
                fill={col}
                fontSize={7}
                fontFamily="ui-monospace, monospace"
                opacity={0.6}
              >
                {p.dir > 0 ? "▲" : "▼"}
              </text>
            )}
          </g>
        );
      })}
    </g>
  );
}

// Family toggle chips. Each chip is a button that flips that family's
// visibility on/off. Lives next to the symbol/timeframe controls so the
// operator can dim noisy families without scrolling. State is owned by
// the parent so the SVG layer can filter detections accordingly.
// Family visibility: "off" = hidden, "on" = formation lines only,
// "detail" = formation + Entry/TP/SL overlay lines + sub-buttons.
type FamilyMode = "off" | "on" | "detail";

const TIMEFRAMES = ["1m", "3m", "5m", "15m", "30m", "1h", "4h", "1d", "1w", "1M"];

function TimeframeBar({
  active,
  onChange,
}: {
  active: string;
  onChange: (tf: string) => void;
}) {
  return (
    <div className="flex items-center gap-0.5 rounded bg-zinc-900 p-0.5">
      {TIMEFRAMES.map((tf) => (
        <button
          key={tf}
          type="button"
          onClick={() => onChange(tf)}
          className={`rounded px-2 py-1 text-xs font-medium transition ${
            tf === active
              ? "bg-zinc-700 text-zinc-100"
              : "text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300"
          }`}
        >
          {tf}
        </button>
      ))}
    </div>
  );
}

const DETAIL_SUB_BUTTONS = [
  { key: "entry_tp_sl", label: "Entry / TP / SL", icon: "⊞" },
  { key: "fib_levels", label: "Fib Levels", icon: "φ" },
  { key: "measured_move", label: "Measured Move", icon: "⟷" },
  { key: "invalidation", label: "Invalidation Zone", icon: "✕" },
];

function FamilyToggles({
  modes,
  onCycle,
  detailLayers,
  onToggleLayer,
}: {
  modes: Record<string, FamilyMode>;
  onCycle: (family: string) => void;
  detailLayers?: Record<string, Set<string>>;
  onToggleLayer?: (family: string, layer: string) => void;
}) {
  const entries = Object.entries(FAMILY_COLORS).filter(([k]) => k !== "custom");
  return (
    <div className="flex flex-col gap-1">
      <div className="flex flex-wrap items-center gap-1.5">
        {entries.map(([family, color]) => {
          const mode = modes[family] ?? "on";
          const isOff = mode === "off";
          const isDetail = mode === "detail";
          return (
            <button
              key={family}
              type="button"
              onClick={() => onCycle(family)}
              className="flex items-center gap-1.5 rounded border px-2 py-1 text-xs uppercase tracking-wide transition"
              style={{
                borderColor: isOff ? "rgb(63 63 70)" : color,
                background: isOff
                  ? "rgb(24 24 27)"
                  : isDetail
                  ? `${color}44`
                  : `${color}22`,
                color: isOff ? "rgb(113 113 122)" : color,
              }}
              title={
                isOff ? `${family}: gizli` : isDetail ? `${family}: detay (Entry/TP/SL)` : `${family}: görünür`
              }
            >
              <span
                className="inline-block h-2 w-3 rounded-sm"
                style={{ background: isOff ? "rgb(63 63 70)" : color }}
              />
              {family}
              {isDetail && (
                <span className="ml-0.5 text-[9px] opacity-70">▸detay</span>
              )}
            </button>
          );
        })}
      </div>
      {/* Sub-menu panel — opens below when any family is in detail mode */}
      {entries.some(([f]) => (modes[f] ?? "on") === "detail") && (
        <div className="flex flex-wrap items-center gap-1 rounded border border-zinc-700/60 bg-zinc-900/80 px-2 py-1.5">
          {entries
            .filter(([f]) => (modes[f] ?? "on") === "detail")
            .map(([family, color]) => {
              const layers = detailLayers?.[family] ?? new Set(["entry_tp_sl"]);
              return (
                <div key={family} className="flex items-center gap-1">
                  <span
                    className="text-[9px] font-bold uppercase tracking-widest"
                    style={{ color }}
                  >
                    {family}
                  </span>
                  {DETAIL_SUB_BUTTONS.map((btn) => {
                    const active = layers.has(btn.key);
                    return (
                      <button
                        key={btn.key}
                        type="button"
                        onClick={() => onToggleLayer?.(family, btn.key)}
                        className="rounded border px-1.5 py-0.5 text-[10px] transition"
                        style={{
                          borderColor: active ? color : "rgb(63 63 70)",
                          background: active ? `${color}33` : "transparent",
                          color: active ? color : "rgb(113 113 122)",
                        }}
                        title={btn.label}
                      >
                        {btn.icon} {btn.label}
                      </button>
                    );
                  })}
                  <span className="mx-1 h-3 w-px bg-zinc-700" />
                </div>
              );
            })}
        </div>
      )}
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
  // Vertical pan/zoom: yOffset shifts price center, yZoom magnifies.
  const [yOffset, setYOffset] = useState(0);
  const [yZoom, setYZoom] = useState(1);
  const [showZigzag, setShowZigzag] = useState(false);
  const [showVolume, setShowVolume] = useState(true);
  // Per-family visibility. Defaults to all-on; clicking a chip in the
  // top toolbar flips that family. Sparse map (missing key = visible)
  // so adding new families needs no migration here.
  // 3-state family toggle: off → on → detail → off
  const [familyModes, setFamilyModes] = useState<Record<string, FamilyMode>>({});
  const [detailLayers, setDetailLayers] = useState<Record<string, Set<string>>>({});
  const cycleFamily = (family: string) =>
    setFamilyModes((prev) => {
      const cur = prev[family] ?? "on";
      const next: FamilyMode = cur === "off" ? "on" : cur === "on" ? "detail" : "off";
      // Auto-enable entry_tp_sl when entering detail mode
      if (next === "detail") {
        setDetailLayers((dl) => ({ ...dl, [family]: new Set(["entry_tp_sl"]) }));
      }
      return { ...prev, [family]: next };
    });
  const toggleLayer = (family: string, layer: string) =>
    setDetailLayers((prev) => {
      const cur = new Set(prev[family] ?? ["entry_tp_sl"]);
      if (cur.has(layer)) cur.delete(layer);
      else cur.add(layer);
      return { ...prev, [family]: cur };
    });
  const fetchingOlderRef = useRef(false);
  const dragRef = useRef<{
    startX: number;
    startY: number;
    startOffset: number;
    startYOffset: number;
    pxPerCandle: number;
    pricePerPx: number;
  } | null>(null);
  const svgRef = useRef<SVGSVGElement | null>(null);

  // Reset paging + viewport whenever the symbol/tf changes.
  useEffect(() => {
    setOlderPages([]);
    setViewOffsetFromEnd(0);
    setViewSize(DEFAULT_VIEW_SIZE);
    setYOffset(0);
    setYZoom(1);
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
  const scale = useMemo(() => buildScale(visibleCandles, yOffset, yZoom), [visibleCandles, yOffset, yZoom]);
  const visibleDetections = useMemo(
    () =>
      merged
        ? merged.detections.filter((d) => (familyModes[d.family] ?? "on") !== "off")
        : [],
    [merged, familyModes],
  );

  // Wyckoff structure overlay (Faz 10)
  const wyckoffQuery = useQuery({
    queryKey: ["v2", "wyckoff", "overlay", debounced.symbol, debounced.timeframe],
    queryFn: () =>
      apiFetch<{ overlay: WyckoffOverlayData | null }>(
        `/v2/wyckoff/overlay/${debounced.symbol}/${debounced.timeframe}`,
      ),
    refetchInterval: 30_000,
  });
  const wyckoffOverlay = wyckoffQuery.data?.overlay ?? null;

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
  const innerH = H - PAD_T - PAD_B;
  const onMouseDown = (e: React.MouseEvent) => {
    if (effectiveViewSize === 0) return;
    const priceSpan = scale.max - scale.min || 1;
    dragRef.current = {
      startX: e.clientX,
      startY: e.clientY,
      startOffset: clampedOffset,
      startYOffset: yOffset,
      pxPerCandle: innerW / effectiveViewSize,
      // Map SVG inner height to actual rendered price range
      pricePerPx: priceSpan / innerH,
    };
    setIsDragging(true);
  };
  const onMouseMove = (e: React.MouseEvent) => {
    const drag = dragRef.current;
    if (!drag) return;
    const dx = e.clientX - drag.startX;
    const dy = e.clientY - drag.startY;
    // Horizontal pan (candles)
    const candleDelta = Math.round(dx / drag.pxPerCandle);
    const next = Math.max(0, Math.min(maxOffset, drag.startOffset + candleDelta));
    setViewOffsetFromEnd(next);
    // Vertical pan (price) — dragging up = higher prices visible
    const priceDelta = dy * drag.pricePerPx;
    setYOffset(drag.startYOffset + priceDelta);
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
    if (e.ctrlKey || e.metaKey) {
      // Ctrl+scroll = vertical (price) zoom
      const factor = e.deltaY > 0 ? 1 / ZOOM_FACTOR : ZOOM_FACTOR;
      setYZoom((prev) => Math.max(0.1, Math.min(20, prev * factor)));
    } else {
      // Plain scroll = horizontal (time) zoom
      zoomBy(e.deltaY > 0 ? ZOOM_FACTOR : 1 / ZOOM_FACTOR);
    }
  };
  const resetView = () => {
    setViewSize(DEFAULT_VIEW_SIZE);
    setViewOffsetFromEnd(0);
    setYOffset(0);
    setYZoom(1);
  };

  const showControls = hoverContainer && !isDragging;

  return (
    <div className="space-y-4">
      <div className="space-y-2 rounded-lg border border-zinc-800 bg-zinc-900/60 p-3 text-sm">
        {/* Row 1: Venue + Symbol + Timeframe buttons */}
        <div className="flex flex-wrap items-center gap-3">
          <div className="flex items-center gap-2">
            <input
              value={form.venue}
              onChange={(e) => setForm({ ...form, venue: e.target.value })}
              className="w-24 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-xs text-zinc-100"
              placeholder="venue"
            />
            <input
              value={form.symbol}
              onChange={(e) => setForm({ ...form, symbol: e.target.value })}
              className="w-28 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 font-mono text-xs text-zinc-100"
              placeholder="symbol"
            />
          </div>
          <TimeframeBar
            active={form.timeframe}
            onChange={(tf) => setForm({ ...form, timeframe: tf })}
          />
        </div>
        {/* Row 2: Family toggles (3-state: off → on → detail) */}
        <div className="flex items-center justify-between">
          <FamilyToggles modes={familyModes} onCycle={cycleFamily} detailLayers={detailLayers} onToggleLayer={toggleLayer} />
          <div className="text-[10px] text-zinc-600">
            sürükle = pan · scroll = zoom · Ctrl+scroll = fiyat zoom
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
              {wyckoffOverlay && (familyModes["wyckoff"] ?? "on") !== "off" && (
                <WyckoffStructureOverlay data={wyckoffOverlay} scale={scale} />
              )}
              <Detections
                detections={visibleDetections}
                candles={visibleCandles}
                scale={scale}
                hovered={hovered}
                onHover={setHovered}
                familyModes={familyModes}
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
