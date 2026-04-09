import { FormEvent, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { RegimeHud, RegimeKind } from "../lib/types";

const DEFAULTS = { venue: "binance", symbol: "BTCUSDT", timeframe: "1h" };

// Color registry for the kind pill so the rest of the page reads as
// data, not branching logic.
const KIND_BADGE: Record<RegimeKind, string> = {
  trending_up: "bg-emerald-500/20 text-emerald-300 border-emerald-500/40",
  trending_down: "bg-red-500/20 text-red-300 border-red-500/40",
  ranging: "bg-sky-500/15 text-sky-300 border-sky-500/30",
  squeeze: "bg-amber-500/15 text-amber-300 border-amber-500/40",
  volatile: "bg-fuchsia-500/15 text-fuchsia-300 border-fuchsia-500/40",
  uncertain: "bg-zinc-700/40 text-zinc-300 border-zinc-700",
};

function Pill({ kind }: { kind: RegimeKind }) {
  return (
    <span className={`rounded border px-2 py-0.5 text-xs uppercase ${KIND_BADGE[kind]}`}>
      {kind.replace("_", " ")}
    </span>
  );
}

function Stat({ label, value }: { label: string; value: string | number }) {
  return (
    <div>
      <div className="text-xs uppercase text-zinc-500">{label}</div>
      <div className="mt-0.5 font-mono text-sm text-zinc-100">{value}</div>
    </div>
  );
}

export function Regime() {
  const [form, setForm] = useState(DEFAULTS);
  const [submitted, setSubmitted] = useState(DEFAULTS);

  const query = useQuery({
    queryKey: ["v2", "regime", submitted],
    queryFn: () =>
      apiFetch<RegimeHud>(
        `/v2/regime/${submitted.venue}/${submitted.symbol}/${submitted.timeframe}`,
      ),
    refetchInterval: 10_000,
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

      {query.isLoading && <div className="text-sm text-zinc-400">Loading regime…</div>}
      {query.isError && (
        <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
          Failed: {(query.error as Error).message}
        </div>
      )}
      {query.data && (
        <>
          <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-5">
            {query.data.current ? (
              <>
                <div className="flex items-center justify-between">
                  <Pill kind={query.data.current.kind} />
                  <div className="text-xs text-zinc-500">
                    {query.data.current.at}
                  </div>
                </div>
                <div className="mt-4 grid grid-cols-2 gap-4 md:grid-cols-3 lg:grid-cols-6">
                  <Stat label="Trend" value={query.data.current.trend_strength} />
                  <Stat label="Confidence" value={query.data.current.confidence.toFixed(2)} />
                  <Stat label="ADX" value={query.data.current.adx} />
                  <Stat label="BB width" value={query.data.current.bb_width} />
                  <Stat label="ATR %" value={query.data.current.atr_pct} />
                  <Stat label="Choppiness" value={query.data.current.choppiness} />
                </div>
              </>
            ) : (
              <div className="text-sm text-zinc-400">Engine warming up…</div>
            )}
          </div>

          <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-4">
            <div className="mb-3 text-xs uppercase tracking-wide text-zinc-500">
              History (newest last)
            </div>
            {query.data.history.length === 0 ? (
              <div className="text-sm text-zinc-500">No history yet.</div>
            ) : (
              <div className="flex flex-wrap gap-2">
                {query.data.history.map((p) => (
                  <div
                    key={p.at}
                    className="flex flex-col items-center gap-1 rounded border border-zinc-800 px-2 py-1"
                    title={`${p.at} · conf ${p.confidence.toFixed(2)}`}
                  >
                    <Pill kind={p.kind} />
                    <span className="font-mono text-[10px] text-zinc-500">
                      {p.confidence.toFixed(2)}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="text-xs text-zinc-500">Generated at {query.data.generated_at}</div>
        </>
      )}
    </div>
  );
}
