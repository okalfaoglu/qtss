import { Fragment } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type {
  StrategyCard,
  StrategyManagerView,
  StrategyParam,
  StrategyStatus,
} from "../lib/types";

// Status pill registry — adding a new lifecycle state means one row
// here, no scattered ternaries.
const STATUS_BADGE: Record<StrategyStatus, string> = {
  active: "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  paused: "bg-amber-500/15 text-amber-300 border-amber-500/30",
  disabled: "bg-zinc-700/40 text-zinc-400 border-zinc-700",
};

function paramValue(p: StrategyParam): string {
  switch (p.kind) {
    case "number":
      return p.value.toString();
    case "integer":
      return p.value.toString();
    case "bool":
      return p.value ? "true" : "false";
    case "text":
      return p.value;
  }
}

function fmtTs(iso: string | null): string {
  if (!iso) return "—";
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

function Card({ card }: { card: StrategyCard }) {
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-4">
      <div className="flex items-baseline justify-between">
        <div>
          <div className="text-sm font-semibold text-zinc-100">{card.label}</div>
          <div className="text-xs text-zinc-500">
            {card.id} <span className="text-zinc-700">·</span> {card.evaluator}
          </div>
        </div>
        <span
          className={`rounded border px-2 py-0.5 text-xs uppercase ${STATUS_BADGE[card.status]}`}
        >
          {card.status}
        </span>
      </div>

      <div className="mt-4 grid grid-cols-2 gap-x-4 gap-y-1 text-xs">
        <div className="text-zinc-500">Signals seen</div>
        <div className="text-right font-mono text-zinc-100">{card.signals_seen}</div>
        <div className="text-zinc-500">Intents emitted</div>
        <div className="text-right font-mono text-zinc-100">{card.intents_emitted}</div>
        <div className="text-zinc-500">Last signal</div>
        <div className="text-right font-mono text-zinc-300">{fmtTs(card.last_signal_at)}</div>
        <div className="text-zinc-500">Last intent</div>
        <div className="text-right font-mono text-zinc-300">{fmtTs(card.last_intent_at)}</div>
      </div>

      {card.params.length > 0 && (
        <div className="mt-4 border-t border-zinc-800 pt-3">
          <div className="mb-2 text-xs uppercase tracking-wide text-zinc-500">Params</div>
          <div className="grid grid-cols-2 gap-x-4 gap-y-1 text-xs">
            {card.params.map((p) => (
              <Fragment key={p.key}>
                <div className="text-zinc-400">{p.key}</div>
                <div className="text-right font-mono text-zinc-100">{paramValue(p)}</div>
              </Fragment>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export function Strategies() {
  const query = useQuery({
    queryKey: ["v2", "strategies"],
    queryFn: () => apiFetch<StrategyManagerView>("/v2/strategies"),
    refetchInterval: 5_000,
  });

  if (query.isLoading) {
    return <div className="text-sm text-zinc-400">Loading strategies…</div>;
  }
  if (query.isError) {
    return (
      <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
        Failed to load strategies: {(query.error as Error).message}
      </div>
    );
  }
  const view = query.data!;

  return (
    <div className="space-y-4">
      <div className="flex items-baseline justify-between">
        <div className="text-xs uppercase tracking-wide text-zinc-500">
          Strategy manager — {view.strategies.length} registered
        </div>
        <div className="text-xs text-zinc-500">Generated at {view.generated_at}</div>
      </div>

      {view.strategies.length === 0 ? (
        <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 px-4 py-6 text-sm text-zinc-500">
          No strategies registered.
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          {view.strategies.map((s) => (
            <Card key={s.id} card={s} />
          ))}
        </div>
      )}
    </div>
  );
}
