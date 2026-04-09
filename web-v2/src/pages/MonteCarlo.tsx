import { FormEvent, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { MonteCarloFan } from "../lib/types";

// Default symbol set comes from a small look-up table rather than
// scattered defaults. The user can override every field on the form;
// the form's purpose is to make first-paint useful without server work.
const DEFAULTS = {
  venue: "binance",
  symbol: "BTCUSDT",
  timeframe: "1h",
  horizon: 30,
  paths: 500,
};

interface FormState {
  venue: string;
  symbol: string;
  timeframe: string;
  horizon: number;
  paths: number;
}

// Build the SVG path for one band's polyline. Anchor is at x=0, the
// band's values fill x=1..N. We map indices to the [0, width] range
// and prices to [height, 0] (SVG y is inverted).
function buildPolyline(
  values: number[],
  anchor: number,
  yMin: number,
  yMax: number,
  width: number,
  height: number,
): string {
  const series = [anchor, ...values];
  const xStep = width / Math.max(1, series.length - 1);
  const ySpan = yMax - yMin || 1;
  return series
    .map((v, i) => {
      const x = i * xStep;
      const y = height - ((v - yMin) / ySpan) * height;
      return `${i === 0 ? "M" : "L"} ${x.toFixed(2)} ${y.toFixed(2)}`;
    })
    .join(" ");
}

// Build a closed area path between two bands (lower to upper) for fan
// fill regions.
function buildAreaPath(
  lower: number[],
  upper: number[],
  anchor: number,
  yMin: number,
  yMax: number,
  width: number,
  height: number,
): string {
  const lowerS = [anchor, ...lower];
  const upperS = [anchor, ...upper];
  const xStep = width / Math.max(1, lowerS.length - 1);
  const ySpan = yMax - yMin || 1;
  const toY = (v: number) => height - ((v - yMin) / ySpan) * height;
  const fwd = upperS
    .map((v, i) => `${i === 0 ? "M" : "L"} ${(i * xStep).toFixed(2)} ${toY(v).toFixed(2)}`)
    .join(" ");
  const back = lowerS
    .map((_, i) => {
      const idx = lowerS.length - 1 - i;
      const x = idx * xStep;
      const y = toY(lowerS[idx]);
      return `L ${x.toFixed(2)} ${y.toFixed(2)}`;
    })
    .join(" ");
  return `${fwd} ${back} Z`;
}

function FanChart({ fan }: { fan: MonteCarloFan }) {
  const width = 720;
  const height = 320;

  const anchor = Number(fan.anchor_price);
  const bandsNumeric = fan.bands.map((b) => ({
    percentile: b.percentile,
    values: b.values.map(Number),
  }));

  const allValues = bandsNumeric.flatMap((b) => b.values).concat(anchor);
  const yMin = Math.min(...allValues);
  const yMax = Math.max(...allValues);

  // Pair symmetric percentiles around the median to draw filled areas.
  // Sorted ascending; the median is the line we draw on top.
  const sorted = [...bandsNumeric].sort((a, b) => a.percentile - b.percentile);
  const median = sorted.find((b) => b.percentile === 50);
  const pairs: Array<[(typeof sorted)[0], (typeof sorted)[0]]> = [];
  for (let i = 0; i < sorted.length / 2; i++) {
    const lo = sorted[i];
    const hi = sorted[sorted.length - 1 - i];
    if (lo.percentile < hi.percentile) pairs.push([lo, hi]);
  }

  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      className="h-80 w-full rounded border border-zinc-800 bg-zinc-950"
    >
      {pairs.map(([lo, hi], i) => (
        <path
          key={`${lo.percentile}-${hi.percentile}`}
          d={buildAreaPath(lo.values, hi.values, anchor, yMin, yMax, width, height)}
          fill="rgb(16 185 129)"
          fillOpacity={0.08 + i * 0.07}
        />
      ))}
      {median && (
        <path
          d={buildPolyline(median.values, anchor, yMin, yMax, width, height)}
          fill="none"
          stroke="rgb(52 211 153)"
          strokeWidth={2}
        />
      )}
    </svg>
  );
}

export function MonteCarlo() {
  const [form, setForm] = useState<FormState>(DEFAULTS);
  const [submitted, setSubmitted] = useState<FormState>(DEFAULTS);

  const query = useQuery({
    queryKey: ["v2", "montecarlo", submitted],
    queryFn: () => {
      const params = new URLSearchParams({
        horizon: String(submitted.horizon),
        paths: String(submitted.paths),
      });
      return apiFetch<MonteCarloFan>(
        `/v2/montecarlo/${submitted.venue}/${submitted.symbol}/${submitted.timeframe}?${params}`,
      );
    },
  });

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
        <label className="flex flex-col gap-1">
          <span className="text-xs uppercase text-zinc-500">Horizon</span>
          <input
            type="number"
            min={1}
            value={form.horizon}
            onChange={(e) => setForm({ ...form, horizon: Number(e.target.value) })}
            className="w-20 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 font-mono text-zinc-100"
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-xs uppercase text-zinc-500">Paths</span>
          <input
            type="number"
            min={1}
            value={form.paths}
            onChange={(e) => setForm({ ...form, paths: Number(e.target.value) })}
            className="w-24 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 font-mono text-zinc-100"
          />
        </label>
        <button
          type="submit"
          className="rounded bg-emerald-500 px-3 py-1.5 text-sm font-medium text-zinc-950 hover:bg-emerald-400"
        >
          Run
        </button>
      </form>

      {query.isLoading && <div className="text-sm text-zinc-400">Simulating…</div>}
      {query.isError && (
        <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
          Failed to run Monte Carlo: {(query.error as Error).message}
        </div>
      )}
      {query.data && (
        <>
          <div className="text-xs text-zinc-500">
            anchor {query.data.anchor_price} · {query.data.paths_simulated} paths ·
            horizon {query.data.horizon_bars} bars
          </div>
          <FanChart fan={query.data} />
          <div className="text-xs text-zinc-500">Generated at {query.data.generated_at}</div>
        </>
      )}
    </div>
  );
}
