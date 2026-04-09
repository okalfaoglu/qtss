import { FormEvent, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { CandleBar, ChartWorkspace, DetectionOverlay } from "../lib/types";

const DEFAULTS = { venue: "binance", symbol: "BTCUSDT", timeframe: "1h" };

// Layout constants. Kept here (single source of truth) so the SVG
// math below stays one place to edit if we ever resize the chart.
const W = 960;
const H = 420;
const PAD_L = 56;
const PAD_R = 16;
const PAD_T = 12;
const PAD_B = 24;

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

function Candles({ candles, scale }: { candles: CandleBar[]; scale: PriceScale }) {
  if (candles.length === 0) return null;
  const innerW = W - PAD_L - PAD_R;
  const step = innerW / candles.length;
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

function Detections({
  detections,
  candles,
  scale,
}: {
  detections: DetectionOverlay[];
  candles: CandleBar[];
  scale: PriceScale;
}) {
  if (detections.length === 0 || candles.length === 0) return null;
  const innerW = W - PAD_L - PAD_R;
  const step = innerW / candles.length;
  // Map anchor time to nearest candle index by linear scan; the
  // candle list is small enough that an interval tree would be overkill.
  const tStart = new Date(candles[0].open_time).getTime();
  const tEnd = new Date(candles[candles.length - 1].open_time).getTime();
  const span = tEnd - tStart || 1;
  return (
    <g>
      {detections.map((d) => {
        const t = new Date(d.anchor_time).getTime();
        if (t < tStart || t > tEnd) return null;
        const i = ((t - tStart) / span) * (candles.length - 1);
        const x = PAD_L + i * step + step / 2;
        const y = scale.toY(Number(d.anchor_price));
        return (
          <g key={d.id}>
            <circle cx={x} cy={y} r={4} fill="rgb(250 204 21)" stroke="rgb(24 24 27)" />
            <text
              x={x + 6}
              y={y - 6}
              fontSize={10}
              fill="rgb(250 204 21)"
              fontFamily="ui-monospace, monospace"
            >
              {d.label}
            </text>
          </g>
        );
      })}
    </g>
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

export function Chart() {
  const [form, setForm] = useState(DEFAULTS);
  const [submitted, setSubmitted] = useState(DEFAULTS);

  const query = useQuery({
    queryKey: ["v2", "chart", submitted],
    queryFn: () =>
      apiFetch<ChartWorkspace>(
        `/v2/chart/${submitted.venue}/${submitted.symbol}/${submitted.timeframe}`,
      ),
    refetchInterval: 10_000,
  });

  const scale = useMemo(() => buildScale(query.data?.candles ?? []), [query.data]);

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(form);
  };

  return (
    <div className="space-y-4">
      <form
        onSubmit={handleSubmit}
        className="flex flex-wrap items-end gap-3 rounded-lg border border-zinc-800 bg-zinc-900/60 p-4 text-sm"
      >
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
        <button
          type="submit"
          className="rounded bg-emerald-500 px-3 py-1.5 text-sm font-medium text-zinc-950 hover:bg-emerald-400"
        >
          Load
        </button>
      </form>

      {query.isLoading && <div className="text-sm text-zinc-400">Loading chart…</div>}
      {query.isError && (
        <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
          Failed: {(query.error as Error).message}
        </div>
      )}
      {query.data && (
        <>
          <div className="text-xs text-zinc-500">
            {query.data.candles.length} candles · {query.data.detections.length} detections ·
            {" "}
            {query.data.positions.length} open positions · {query.data.open_orders.length} working orders
          </div>
          <svg
            viewBox={`0 0 ${W} ${H}`}
            className="w-full rounded border border-zinc-800 bg-zinc-950"
          >
            <Axis scale={scale} />
            <Candles candles={query.data.candles} scale={scale} />
            <Detections
              detections={query.data.detections}
              candles={query.data.candles}
              scale={scale}
            />
          </svg>

          {query.data.detections.length > 0 && (
            <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-4">
              <div className="mb-2 text-xs uppercase tracking-wide text-zinc-500">
                Detections
              </div>
              <table className="w-full text-sm">
                <thead className="text-xs uppercase text-zinc-500">
                  <tr>
                    <th className="px-2 py-1 text-left">Kind</th>
                    <th className="px-2 py-1 text-left">Label</th>
                    <th className="px-2 py-1 text-left">When</th>
                    <th className="px-2 py-1 text-right">Price</th>
                    <th className="px-2 py-1 text-right">Conf</th>
                  </tr>
                </thead>
                <tbody className="font-mono text-zinc-100">
                  {query.data.detections.map((d) => (
                    <tr key={d.id} className="border-t border-zinc-800/60">
                      <td className="px-2 py-1">{d.kind}</td>
                      <td className="px-2 py-1">{d.label}</td>
                      <td className="px-2 py-1 text-zinc-400">{d.anchor_time}</td>
                      <td className="px-2 py-1 text-right">{d.anchor_price}</td>
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
