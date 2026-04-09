import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { AuditView } from "../lib/types";

// Status code → tailwind classes. The audit table is dense so the
// colour is the only thing the eye scans for; everything else is text.
function statusColor(code: number): string {
  if (code >= 500) return "bg-red-500/20 text-red-300 border-red-500/40";
  if (code >= 400) return "bg-amber-500/15 text-amber-300 border-amber-500/40";
  if (code >= 300) return "bg-sky-500/15 text-sky-300 border-sky-500/30";
  return "bg-emerald-500/15 text-emerald-300 border-emerald-500/30";
}

export function Audit() {
  const query = useQuery({
    queryKey: ["v2", "audit"],
    queryFn: () => apiFetch<AuditView>("/v2/audit?limit=200"),
    refetchInterval: 15_000,
  });

  if (query.isLoading) {
    return <div className="text-sm text-zinc-400">Loading audit log…</div>;
  }
  if (query.isError) {
    return (
      <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
        Failed: {(query.error as Error).message}
      </div>
    );
  }
  const view = query.data!;

  return (
    <div className="space-y-3">
      <div className="flex items-baseline justify-between">
        <div className="text-xs uppercase tracking-wide text-zinc-500">
          Audit log — {view.entries.length} entries
        </div>
        <div className="text-xs text-zinc-500">Generated at {view.generated_at}</div>
      </div>

      <div className="overflow-hidden rounded-lg border border-zinc-800 bg-zinc-900/60">
        {view.entries.length === 0 ? (
          <div className="px-4 py-6 text-sm text-zinc-500">No audit rows yet.</div>
        ) : (
          <table className="w-full text-sm">
            <thead className="bg-zinc-900/80 text-xs uppercase text-zinc-500">
              <tr>
                <th className="px-3 py-2 text-left">When</th>
                <th className="px-3 py-2 text-left">Method</th>
                <th className="px-3 py-2 text-left">Path</th>
                <th className="px-3 py-2 text-left">Status</th>
                <th className="px-3 py-2 text-left">Kind</th>
                <th className="px-3 py-2 text-left">Roles</th>
                <th className="px-3 py-2 text-left">Details</th>
              </tr>
            </thead>
            <tbody>
              {view.entries.map((e) => (
                <tr key={e.id} className="border-t border-zinc-800/60 align-top">
                  <td className="px-3 py-2 font-mono text-xs text-zinc-400" title={e.at}>
                    {e.at.replace("T", " ").replace(/\.\d+Z$/, "Z")}
                  </td>
                  <td className="px-3 py-2 font-mono text-xs text-zinc-300">{e.method}</td>
                  <td className="px-3 py-2 font-mono text-xs text-zinc-100">{e.path}</td>
                  <td className="px-3 py-2">
                    <span
                      className={`inline-block rounded border px-2 py-0.5 font-mono text-xs ${statusColor(e.status_code)}`}
                    >
                      {e.status_code}
                    </span>
                  </td>
                  <td className="px-3 py-2 text-xs text-zinc-300">{e.kind ?? "—"}</td>
                  <td className="px-3 py-2 text-xs text-zinc-400">
                    {e.roles.length === 0 ? "—" : e.roles.join(", ")}
                  </td>
                  <td className="px-3 py-2 font-mono text-xs text-zinc-500">
                    {e.details_preview ?? "—"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
