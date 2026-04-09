import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";

// Wire types — match crates/qtss-api/src/routes/v2_onchain.rs.
interface OnchainEntry {
  id: string;
  symbol: string;
  computed_at: string;
  derivatives_score: number | null;
  stablecoin_score: number | null;
  chain_score: number | null;
  aggregate_score: number; // 0..1
  direction: string;       // long | short | neutral
  confidence: number;      // 0..1
  details: string[];
}

interface OnchainFeed {
  generated_at: string;
  entries: OnchainEntry[];
}

const DIRECTION_BADGE: Record<string, string> = {
  long: "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  short: "bg-rose-500/15 text-rose-300 border-rose-500/30",
  neutral: "bg-zinc-700/40 text-zinc-300 border-zinc-600/40",
};

const CATEGORY_COLORS: Record<string, string> = {
  derivatives: "bg-sky-400",
  stablecoin: "bg-violet-400",
  chain: "bg-emerald-400",
};

function fmtTime(iso: string): string {
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

// Categories live in [-1, +1]. Map to a 0..100 bar so we can render
// neutral at the centre.
function CategoryBar({ label, value }: { label: string; value: number | null }) {
  const color = CATEGORY_COLORS[label] ?? "bg-zinc-400";
  if (value === null || value === undefined) {
    return (
      <div className="space-y-1">
        <div className="flex items-baseline justify-between text-[11px]">
          <span className="text-zinc-400">{label}</span>
          <span className="font-mono text-zinc-600">n/a</span>
        </div>
        <div className="h-1.5 w-full rounded-full bg-zinc-800" />
      </div>
    );
  }
  const v = Math.max(-1, Math.min(1, value));
  const pct = ((v + 1) * 50).toFixed(0);
  return (
    <div className="space-y-1">
      <div className="flex items-baseline justify-between text-[11px]">
        <span className="text-zinc-300">{label}</span>
        <span className="font-mono text-zinc-400">{v.toFixed(2)}</span>
      </div>
      <div className="relative h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
        <div className="absolute inset-y-0 left-1/2 w-px bg-zinc-600" />
        <div
          className={`absolute top-0 h-full ${color}`}
          style={
            v >= 0
              ? { left: "50%", width: `${(v * 50).toFixed(1)}%` }
              : { right: "50%", width: `${(-v * 50).toFixed(1)}%` }
          }
        />
      </div>
    </div>
  );
}

function OnchainCard({ entry }: { entry: OnchainEntry }) {
  const dirCls = DIRECTION_BADGE[entry.direction] ?? DIRECTION_BADGE.neutral;
  const aggPct = (entry.aggregate_score * 100).toFixed(0);
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="font-mono text-sm text-zinc-100">{entry.symbol}</div>
          <div className="mt-1 flex items-center gap-2">
            <span className={`rounded border px-1.5 py-0.5 text-[10px] uppercase ${dirCls}`}>
              {entry.direction}
            </span>
            <span className="text-[10px] text-zinc-600">
              conf {(entry.confidence * 100).toFixed(0)}%
            </span>
          </div>
        </div>
        <div className="text-right">
          <div className="font-mono text-2xl text-zinc-100">{aggPct}</div>
          <div className="text-[10px] text-zinc-600">aggregate / 100</div>
        </div>
      </div>

      <div className="mt-4 space-y-2">
        <CategoryBar label="derivatives" value={entry.derivatives_score} />
        <CategoryBar label="stablecoin" value={entry.stablecoin_score} />
        <CategoryBar label="chain" value={entry.chain_score} />
      </div>

      {entry.details.length > 0 && (
        <details className="mt-3 text-[11px] text-zinc-400">
          <summary className="cursor-pointer text-zinc-500 hover:text-zinc-300">
            {entry.details.length} signal notes
          </summary>
          <ul className="mt-1 list-disc space-y-0.5 pl-4">
            {entry.details.map((d, i) => (
              <li key={i}>{d}</li>
            ))}
          </ul>
        </details>
      )}

      <div className="mt-3 text-[10px] text-zinc-600">{fmtTime(entry.computed_at)}</div>
    </div>
  );
}

export function Onchain() {
  const [symbol, setSymbol] = useState("");

  const endpoint = useMemo(() => {
    if (symbol.trim()) {
      const params = new URLSearchParams();
      params.set("symbol", symbol.trim().toUpperCase());
      params.set("limit", "200");
      return `/v2/onchain?${params.toString()}`;
    }
    return `/v2/onchain/latest`;
  }, [symbol]);

  const { data, isLoading, error, refetch, isFetching } = useQuery<OnchainFeed>({
    queryKey: ["v2-onchain", endpoint],
    queryFn: () => apiFetch<OnchainFeed>(endpoint),
    refetchInterval: 60_000,
  });

  const entries = data?.entries ?? [];

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold text-zinc-100">Onchain</h1>
          <p className="text-xs text-zinc-500">
            Aggregate onchain pillar — derivatives + stablecoin macro + chain cohort
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
          Symbol (empty = latest per symbol)
          <input
            value={symbol}
            onChange={(e) => setSymbol(e.target.value.toUpperCase())}
            placeholder="BTCUSDT"
            className="w-40 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-sm text-zinc-100 focus:border-emerald-500 focus:outline-none"
          />
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
          No onchain rows yet. Enable the v2 onchain loop with{" "}
          <code className="text-zinc-300">
            UPDATE system_config SET value='true'::jsonb WHERE module='onchain' AND
            config_key='enabled';
          </code>{" "}
          and wait for the next pass (~5 min).
        </div>
      )}

      <div className="grid gap-3 md:grid-cols-2 lg:grid-cols-3">
        {entries.map((e) => (
          <OnchainCard key={e.id} entry={e} />
        ))}
      </div>
    </div>
  );
}
