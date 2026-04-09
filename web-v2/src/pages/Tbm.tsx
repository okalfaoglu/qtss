import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";

// Wire types — match crates/qtss-api/src/routes/v2_tbm.rs.
interface TbmPillar {
  kind: string;
  score: number;
  weight: number;
}

interface TbmEntry {
  id: string;
  detected_at: string;
  exchange: string;
  symbol: string;
  timeframe: string;
  subkind: string;
  state: string;
  mode: string;
  total: number;
  signal: string;
  pillars: TbmPillar[];
  details: string[];
}

interface TbmFeed {
  generated_at: string;
  entries: TbmEntry[];
}

// Static lookups — adding a new signal/state is a one-line change
// (CLAUDE.md #1 spirit on the FE).
const SIGNAL_BADGE: Record<string, string> = {
  VeryStrong: "bg-emerald-500/20 text-emerald-200 border-emerald-400/40",
  Strong: "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  Moderate: "bg-amber-500/15 text-amber-300 border-amber-500/30",
  Weak: "bg-zinc-700/40 text-zinc-300 border-zinc-600/40",
  None: "bg-zinc-800/60 text-zinc-500 border-zinc-700/40",
};

const SUBKIND_BADGE: Record<string, string> = {
  bottom_setup: "bg-sky-500/15 text-sky-300 border-sky-500/30",
  top_setup: "bg-pink-500/15 text-pink-300 border-pink-500/30",
};

const PILLAR_COLORS: Record<string, string> = {
  Momentum: "bg-sky-400",
  Volume: "bg-violet-400",
  Structure: "bg-amber-400",
  Onchain: "bg-emerald-400",
};

function fmtTime(iso: string): string {
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

// Latest entry per (symbol, timeframe) — TBM is a setup detector and
// the dashboard cares about *current* state, not history. The list
// endpoint already returns rows newest-first, so the first occurrence
// of each (symbol, tf) wins.
function latestPerSymbol(entries: TbmEntry[]): TbmEntry[] {
  const seen = new Set<string>();
  const out: TbmEntry[] = [];
  for (const e of entries) {
    const key = `${e.exchange}/${e.symbol}/${e.timeframe}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(e);
  }
  return out;
}

function PillarBar({ pillar }: { pillar: TbmPillar }) {
  const color = PILLAR_COLORS[pillar.kind] ?? "bg-zinc-400";
  const pct = Math.max(0, Math.min(100, pillar.score));
  return (
    <div className="space-y-1">
      <div className="flex items-baseline justify-between text-[11px]">
        <span className="text-zinc-300">{pillar.kind}</span>
        <span className="font-mono text-zinc-400">
          {pct.toFixed(0)} <span className="text-zinc-600">· w {pillar.weight.toFixed(2)}</span>
        </span>
      </div>
      <div className="h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
        <div className={`h-full ${color}`} style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

function TbmCard({ entry }: { entry: TbmEntry }) {
  const signalCls = SIGNAL_BADGE[entry.signal] ?? SIGNAL_BADGE.None;
  const subCls = SUBKIND_BADGE[entry.subkind] ?? "bg-zinc-700/40 text-zinc-300 border-zinc-600/40";
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <span className="font-mono text-sm text-zinc-100">
              {entry.symbol}
              <span className="ml-1 text-zinc-500">{entry.timeframe}</span>
            </span>
            <span className="text-[10px] uppercase tracking-wide text-zinc-600">
              {entry.exchange} · {entry.mode}
            </span>
          </div>
          <div className="mt-1 flex items-center gap-2">
            <span className={`rounded border px-1.5 py-0.5 text-[10px] uppercase ${subCls}`}>
              {entry.subkind.replace("_", " ")}
            </span>
            <span className={`rounded border px-1.5 py-0.5 text-[10px] uppercase ${signalCls}`}>
              {entry.signal}
            </span>
          </div>
        </div>
        <div className="text-right">
          <div className="font-mono text-2xl text-zinc-100">{entry.total.toFixed(0)}</div>
          <div className="text-[10px] text-zinc-600">total</div>
        </div>
      </div>

      <div className="mt-4 space-y-2">
        {entry.pillars.length === 0 ? (
          <div className="text-xs text-zinc-600">no pillar breakdown</div>
        ) : (
          entry.pillars.map((p) => <PillarBar key={p.kind} pillar={p} />)
        )}
      </div>

      {entry.details.length > 0 && (
        <details className="mt-3 text-[11px] text-zinc-400">
          <summary className="cursor-pointer text-zinc-500 hover:text-zinc-300">
            {entry.details.length} pillar notes
          </summary>
          <ul className="mt-1 list-disc space-y-0.5 pl-4">
            {entry.details.map((d, i) => (
              <li key={i}>{d}</li>
            ))}
          </ul>
        </details>
      )}

      <div className="mt-3 text-[10px] text-zinc-600">{fmtTime(entry.detected_at)}</div>
    </div>
  );
}

export function Tbm() {
  const [symbol, setSymbol] = useState("");
  const [timeframe, setTimeframe] = useState("");
  const [direction, setDirection] = useState<"all" | "bottom_setup" | "top_setup">("all");

  const qs = useMemo(() => {
    const params = new URLSearchParams();
    if (symbol) params.set("symbol", symbol);
    if (timeframe) params.set("timeframe", timeframe);
    params.set("limit", "200");
    return params.toString();
  }, [symbol, timeframe]);

  const { data, isLoading, error, refetch, isFetching } = useQuery<TbmFeed>({
    queryKey: ["v2-tbm", qs],
    queryFn: () => apiFetch<TbmFeed>(`/v2/tbm?${qs}`),
    refetchInterval: 60_000,
  });

  const entries = useMemo(() => {
    const all = data?.entries ?? [];
    const filtered =
      direction === "all" ? all : all.filter((e) => e.subkind === direction);
    return latestPerSymbol(filtered);
  }, [data, direction]);

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold text-zinc-100">TBM</h1>
          <p className="text-xs text-zinc-500">
            Top/Bottom Mining — pillar breakdown of the most recent reversal setups
          </p>
        </div>
        <button
          type="button"
          onClick={() => refetch()}
          className="rounded border border-zinc-700 px-3 py-1 text-xs text-zinc-300 hover:border-zinc-500 hover:text-zinc-100"
        >
          {isFetching ? "Refreshing…" : "Refresh"}
        </button>
      </div>

      <div className="flex flex-wrap items-end gap-3 rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
        <label className="flex flex-col gap-1 text-[11px] text-zinc-500">
          Symbol
          <input
            value={symbol}
            onChange={(e) => setSymbol(e.target.value.toUpperCase())}
            placeholder="BTCUSDT"
            className="w-32 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-sm text-zinc-100 focus:border-emerald-500 focus:outline-none"
          />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-zinc-500">
          Timeframe
          <input
            value={timeframe}
            onChange={(e) => setTimeframe(e.target.value)}
            placeholder="1h"
            className="w-20 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-sm text-zinc-100 focus:border-emerald-500 focus:outline-none"
          />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-zinc-500">
          Direction
          <select
            value={direction}
            onChange={(e) => setDirection(e.target.value as typeof direction)}
            className="rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-sm text-zinc-100 focus:border-emerald-500 focus:outline-none"
          >
            <option value="all">All</option>
            <option value="bottom_setup">Bottom</option>
            <option value="top_setup">Top</option>
          </select>
        </label>
      </div>

      {isLoading && <div className="text-sm text-zinc-500">Loading…</div>}
      {error && (
        <div className="rounded border border-rose-500/30 bg-rose-950/20 p-3 text-sm text-rose-300">
          {(error as Error).message}
        </div>
      )}
      {!isLoading && !error && entries.length === 0 && (
        <div className="rounded border border-zinc-800 bg-zinc-900/30 p-6 text-center text-sm text-zinc-500">
          No TBM detections yet. Enable the loop with{" "}
          <code className="text-zinc-300">
            UPSERT system_config (tbm, enabled, {`{"enabled":true}`})
          </code>{" "}
          and wait for the next pass (~60s).
        </div>
      )}

      <div className="grid gap-3 md:grid-cols-2 lg:grid-cols-3">
        {entries.map((e) => (
          <TbmCard key={e.id} entry={e} />
        ))}
      </div>
    </div>
  );
}
