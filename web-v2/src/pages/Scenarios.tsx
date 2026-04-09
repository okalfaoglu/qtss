import { FormEvent, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { ScenarioNode, ScenarioTree } from "../lib/types";

const DEFAULTS = { venue: "binance", symbol: "BTCUSDT", timeframe: "1h" };

// Label color registry — branches share names ("bull", "bear",
// "continuation"...) so the same map covers root + child levels.
const LABEL_COLOR: Record<string, string> = {
  bull: "border-emerald-500/40 text-emerald-300 bg-emerald-500/10",
  bear: "border-red-500/40 text-red-300 bg-red-500/10",
  neutral: "border-sky-500/40 text-sky-300 bg-sky-500/10",
  continuation: "border-emerald-500/30 text-emerald-300 bg-emerald-500/5",
  reversal: "border-amber-500/40 text-amber-300 bg-amber-500/10",
  root: "border-zinc-700 text-zinc-300 bg-zinc-900/60",
};

function colorFor(label: string): string {
  return LABEL_COLOR[label] ?? "border-zinc-700 text-zinc-300 bg-zinc-900/60";
}

function NodeCard({ node, depth }: { node: ScenarioNode; depth: number }) {
  const pct = (Number(node.probability) * 100).toFixed(1);
  return (
    <div className="space-y-2">
      <div
        className={`rounded-lg border px-3 py-2 text-sm ${colorFor(node.label)}`}
        style={{ marginLeft: depth * 24 }}
      >
        <div className="flex items-baseline justify-between gap-3">
          <div className="font-semibold uppercase tracking-wide">{node.label}</div>
          <div className="font-mono text-xs">{pct}%</div>
        </div>
        <div className="mt-1 text-xs text-zinc-400">{node.trigger}</div>
        <div className="mt-1 font-mono text-xs text-zinc-300">
          {node.target_band.low} … {node.target_band.high}
        </div>
      </div>
      {node.children.map((c) => (
        <NodeCard key={c.id} node={c} depth={depth + 1} />
      ))}
    </div>
  );
}

export function Scenarios() {
  const [form, setForm] = useState(DEFAULTS);
  const [submitted, setSubmitted] = useState(DEFAULTS);

  const query = useQuery({
    queryKey: ["v2", "scenarios", submitted],
    queryFn: () =>
      apiFetch<ScenarioTree>(
        `/v2/scenarios/${submitted.venue}/${submitted.symbol}/${submitted.timeframe}`,
      ),
    refetchInterval: 30_000,
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
        <button
          type="submit"
          className="rounded bg-emerald-500 px-3 py-1.5 text-sm font-medium text-zinc-950 hover:bg-emerald-400"
        >
          Load
        </button>
      </form>

      {query.isLoading && <div className="text-sm text-zinc-400">Loading scenarios…</div>}
      {query.isError && (
        <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
          Failed: {(query.error as Error).message}
        </div>
      )}
      {query.data && (
        <>
          <div className="text-xs text-zinc-500">
            anchor {query.data.anchor_price} · horizon {query.data.horizon_bars} bars
          </div>
          <NodeCard node={query.data.root} depth={0} />
          <div className="text-xs text-zinc-500">Generated at {query.data.generated_at}</div>
        </>
      )}
    </div>
  );
}
