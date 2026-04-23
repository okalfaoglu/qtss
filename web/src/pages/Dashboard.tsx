import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { DashboardSnapshot } from "../lib/types";

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-4">
      <div className="text-xs uppercase tracking-wide text-zinc-500">{title}</div>
      <div className="mt-2 space-y-1 text-sm text-zinc-100">{children}</div>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <span className="text-zinc-400">{label}</span>
      <span className="font-mono text-zinc-100">{value}</span>
    </div>
  );
}

export function Dashboard() {
  const query = useQuery({
    queryKey: ["v2", "dashboard"],
    queryFn: () => apiFetch<DashboardSnapshot>("/v2/dashboard"),
    refetchInterval: 5_000,
  });

  if (query.isLoading) {
    return <div className="text-sm text-zinc-400">Loading dashboard…</div>;
  }
  if (query.isError) {
    return (
      <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
        Failed to load dashboard: {(query.error as Error).message}
      </div>
    );
  }
  const snap = query.data!;

  return (
    <div className="space-y-6">
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <Card title="Portfolio">
          <Row label="Equity" value={snap.portfolio.equity} />
          <Row label="Cash" value={snap.portfolio.cash} />
          <Row label="Realized PnL" value={snap.portfolio.realized_pnl} />
          <Row label="Unrealized PnL" value={snap.portfolio.unrealized_pnl} />
          <Row label="Open positions" value={snap.portfolio.open_position_count} />
          <Row label="Open notional" value={snap.portfolio.open_notional} />
        </Card>
        <Card title="Risk">
          <Row label="Drawdown %" value={snap.risk.drawdown_pct} />
          <Row label="Daily loss %" value={snap.risk.daily_loss_pct} />
          <Row label="Leverage" value={snap.risk.leverage} />
          <Row label="Killswitch" value={snap.risk.killswitch_armed ? "ARMED" : "off"} />
          <Row label="Any breach" value={snap.risk.any_breached ? "yes" : "no"} />
        </Card>
      </div>

      <div className="rounded-lg border border-zinc-800 bg-zinc-900/60">
        <div className="border-b border-zinc-800 px-4 py-3 text-xs uppercase tracking-wide text-zinc-500">
          Open positions
        </div>
        {snap.open_positions.length === 0 ? (
          <div className="px-4 py-6 text-sm text-zinc-500">No open positions.</div>
        ) : (
          <table className="w-full text-sm">
            <thead className="text-xs uppercase text-zinc-500">
              <tr>
                <th className="px-4 py-2 text-left">Symbol</th>
                <th className="px-4 py-2 text-left">Venue</th>
                <th className="px-4 py-2 text-left">Side</th>
                <th className="px-4 py-2 text-right">Qty</th>
                <th className="px-4 py-2 text-right">Entry</th>
                <th className="px-4 py-2 text-right">Mark</th>
                <th className="px-4 py-2 text-right">uPnL</th>
                <th className="px-4 py-2 text-right">uPnL %</th>
              </tr>
            </thead>
            <tbody className="font-mono text-zinc-100">
              {snap.open_positions.map((p, i) => (
                <tr key={i} className="border-t border-zinc-800/60">
                  <td className="px-4 py-2">{p.symbol}</td>
                  <td className="px-4 py-2">{p.venue}</td>
                  <td className="px-4 py-2">{p.side}</td>
                  <td className="px-4 py-2 text-right">{p.quantity}</td>
                  <td className="px-4 py-2 text-right">{p.entry_price}</td>
                  <td className="px-4 py-2 text-right">{p.mark_price}</td>
                  <td className="px-4 py-2 text-right">{p.unrealized_pnl}</td>
                  <td className="px-4 py-2 text-right">{p.unrealized_pnl_pct}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <div className="text-xs text-zinc-500">Generated at {snap.generated_at}</div>
    </div>
  );
}
