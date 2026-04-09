import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { RiskGauge, RiskHud } from "../lib/types";

// Map a 0..1 utilization fraction to a tailwind color class. We do not
// hardcode thresholds anywhere else — this single function owns the
// "how full is the bar" → "what colour" decision.
function utilizationColor(u: number): string {
  if (u >= 1) return "bg-red-500";
  if (u >= 0.8) return "bg-amber-500";
  if (u >= 0.5) return "bg-yellow-500";
  return "bg-emerald-500";
}

function GaugeRow({ gauge }: { gauge: RiskGauge }) {
  const u = Math.max(0, Math.min(1, Number(gauge.utilization)));
  const pct = (u * 100).toFixed(1);
  return (
    <div className="space-y-1">
      <div className="flex items-baseline justify-between text-sm">
        <span className="text-zinc-300">{gauge.label}</span>
        <span className="font-mono text-zinc-400">
          {gauge.value} / {gauge.cap}
          <span className="ml-2 text-zinc-500">({pct}%)</span>
        </span>
      </div>
      <div className="h-2 w-full overflow-hidden rounded bg-zinc-800">
        <div
          className={`h-full ${utilizationColor(u)} transition-all`}
          style={{ width: `${pct}%` }}
        />
      </div>
      {gauge.breached && (
        <div className="text-xs text-red-400">cap exceeded</div>
      )}
    </div>
  );
}

export function Risk() {
  const query = useQuery({
    queryKey: ["v2", "risk"],
    queryFn: () => apiFetch<RiskHud>("/v2/risk"),
    refetchInterval: 5_000,
  });

  if (query.isLoading) {
    return <div className="text-sm text-zinc-400">Loading risk HUD…</div>;
  }
  if (query.isError) {
    return (
      <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
        Failed to load risk HUD: {(query.error as Error).message}
      </div>
    );
  }
  const hud = query.data!;

  return (
    <div className="space-y-6">
      {hud.kill_switch_armed && (
        <div className="rounded border border-red-700 bg-red-950/60 px-4 py-3 text-sm font-semibold text-red-200">
          KILL-SWITCH ARMED — hard drawdown threshold breached
        </div>
      )}
      {hud.any_breached && !hud.kill_switch_armed && (
        <div className="rounded border border-amber-700 bg-amber-950/40 px-4 py-3 text-sm text-amber-200">
          One or more soft caps are breached
        </div>
      )}
      {hud.kill_switch_manual && (
        <div className="rounded border border-zinc-700 bg-zinc-900/60 px-4 py-3 text-sm text-zinc-300">
          Manual kill-switch is engaged
        </div>
      )}

      <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-5">
        <div className="mb-4 text-xs uppercase tracking-wide text-zinc-500">
          Risk gauges
        </div>
        <div className="space-y-4">
          {hud.gauges.map((g) => (
            <GaugeRow key={g.label} gauge={g} />
          ))}
        </div>
      </div>

      <div className="text-xs text-zinc-500">Generated at {hud.generated_at}</div>
    </div>
  );
}
