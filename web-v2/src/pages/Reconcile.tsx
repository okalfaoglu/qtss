import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { apiFetch } from "../lib/api";

interface ReconcileReport {
  id: number;
  user_id: string;
  venue: string;
  overall: string;
  position_drifts: unknown;
  order_drifts: unknown;
  position_count: number;
  order_count: number;
  created_at: string;
}

function severityBadge(s: string) {
  const cls: Record<string, string> = {
    none: "bg-emerald-500/20 text-emerald-300",
    soft: "bg-yellow-500/20 text-yellow-300",
    drift: "bg-orange-500/20 text-orange-300",
    critical: "bg-red-500/20 text-red-300",
  };
  return (
    <span
      className={`rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase ${cls[s] ?? "bg-zinc-700 text-zinc-300"}`}
    >
      {s}
    </span>
  );
}

function DetailModal({
  report,
  onClose,
}: {
  report: ReconcileReport;
  onClose: () => void;
}) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="max-h-[80vh] w-full max-w-3xl overflow-y-auto rounded-lg border border-zinc-800 bg-zinc-900 p-5 shadow-2xl"
      >
        <div className="mb-3 flex items-center gap-3">
          <span className="font-mono text-sm text-zinc-100">{report.venue}</span>
          {severityBadge(report.overall)}
          <span className="text-xs text-zinc-500">
            {new Date(report.created_at).toLocaleString()}
          </span>
        </div>

        <div className="space-y-4">
          <div>
            <h3 className="mb-1 text-xs font-semibold uppercase text-zinc-400">
              Position Drifts ({report.position_count})
            </h3>
            <pre className="max-h-48 overflow-auto rounded border border-zinc-800 bg-zinc-950 p-3 font-mono text-xs text-zinc-300">
              {JSON.stringify(report.position_drifts, null, 2)}
            </pre>
          </div>
          <div>
            <h3 className="mb-1 text-xs font-semibold uppercase text-zinc-400">
              Order Drifts ({report.order_count})
            </h3>
            <pre className="max-h-48 overflow-auto rounded border border-zinc-800 bg-zinc-950 p-3 font-mono text-xs text-zinc-300">
              {JSON.stringify(report.order_drifts, null, 2)}
            </pre>
          </div>
        </div>

        <div className="mt-4 flex justify-end">
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-zinc-700 px-3 py-1.5 text-sm text-zinc-300 hover:border-zinc-500"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

export function Reconcile() {
  const [detail, setDetail] = useState<ReconcileReport | null>(null);

  const latestQ = useQuery({
    queryKey: ["v2", "reconcile", "latest"],
    queryFn: () => apiFetch<ReconcileReport[]>("/v2/reconcile/reports/latest"),
    refetchInterval: 60_000,
  });

  const historyQ = useQuery({
    queryKey: ["v2", "reconcile", "history"],
    queryFn: () => apiFetch<ReconcileReport[]>("/v2/reconcile/reports?limit=50"),
    refetchInterval: 60_000,
  });

  return (
    <div className="space-y-6">
      {/* Latest per venue */}
      <div>
        <h2 className="mb-2 text-sm font-semibold text-zinc-200">Latest per Venue</h2>
        {latestQ.isLoading && <div className="text-xs text-zinc-500">Loading…</div>}
        {latestQ.data && latestQ.data.length === 0 && (
          <div className="text-xs text-zinc-500">No reconciliation reports yet.</div>
        )}
        {latestQ.data && latestQ.data.length > 0 && (
          <div className="flex flex-wrap gap-3">
            {latestQ.data.map((r) => (
              <button
                key={r.id}
                type="button"
                onClick={() => setDetail(r)}
                className="flex items-center gap-2 rounded-lg border border-zinc-800 bg-zinc-900/60 px-4 py-3 text-left hover:border-zinc-600"
              >
                <span className="font-mono text-sm text-zinc-100">{r.venue}</span>
                {severityBadge(r.overall)}
                <span className="text-xs text-zinc-500">
                  {new Date(r.created_at).toLocaleString()}
                </span>
              </button>
            ))}
          </div>
        )}
      </div>

      {/* History table */}
      <div>
        <h2 className="mb-2 text-sm font-semibold text-zinc-200">History</h2>
        {historyQ.isLoading && <div className="text-xs text-zinc-500">Loading…</div>}
        {historyQ.isError && (
          <div className="text-xs text-red-300">
            {(historyQ.error as Error).message}
          </div>
        )}
        {historyQ.data && historyQ.data.length > 0 && (
          <table className="w-full text-sm">
            <thead className="text-[10px] uppercase text-zinc-500">
              <tr>
                <th className="px-3 py-2 text-left">Time</th>
                <th className="px-3 py-2 text-left">Venue</th>
                <th className="px-3 py-2 text-left">Severity</th>
                <th className="px-3 py-2 text-left">Pos.</th>
                <th className="px-3 py-2 text-left">Ord.</th>
                <th className="px-3 py-2 text-left" />
              </tr>
            </thead>
            <tbody>
              {historyQ.data.map((r) => (
                <tr key={r.id} className="border-t border-zinc-800/60">
                  <td className="px-3 py-2 font-mono text-xs text-zinc-400">
                    {new Date(r.created_at).toLocaleString()}
                  </td>
                  <td className="px-3 py-2 font-mono text-xs text-zinc-300">{r.venue}</td>
                  <td className="px-3 py-2">{severityBadge(r.overall)}</td>
                  <td className="px-3 py-2 text-xs text-zinc-400">{r.position_count}</td>
                  <td className="px-3 py-2 text-xs text-zinc-400">{r.order_count}</td>
                  <td className="px-3 py-2">
                    <button
                      type="button"
                      onClick={() => setDetail(r)}
                      className="rounded border border-zinc-700 px-2 py-0.5 text-[10px] text-zinc-300 hover:border-blue-500/40 hover:text-blue-300"
                    >
                      Detail
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {detail && <DetailModal report={detail} onClose={() => setDetail(null)} />}
    </div>
  );
}
