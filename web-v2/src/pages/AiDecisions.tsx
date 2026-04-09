import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { AiDecisionStatus, AiDecisionsView } from "../lib/types";

const STATUS_BADGE: Record<AiDecisionStatus, string> = {
  pending: "bg-amber-500/15 text-amber-300 border-amber-500/40",
  approved: "bg-emerald-500/15 text-emerald-300 border-emerald-500/40",
  rejected: "bg-red-500/15 text-red-300 border-red-500/40",
  other: "bg-zinc-700/40 text-zinc-300 border-zinc-700",
};

const FILTERS: Array<{ label: string; value: AiDecisionStatus | "all" }> = [
  { label: "All", value: "all" },
  { label: "Pending", value: "pending" },
  { label: "Approved", value: "approved" },
  { label: "Rejected", value: "rejected" },
];

interface DecideArgs {
  id: string;
  status: "approved" | "rejected";
  admin_note?: string;
}

export function AiDecisions() {
  const qc = useQueryClient();
  const [status, setStatus] = useState<AiDecisionStatus | "all">("all");
  const [error, setError] = useState<string | null>(null);

  const query = useQuery({
    queryKey: ["v2", "ai-decisions", status],
    queryFn: () => {
      const qp = status === "all" ? "" : `?status=${status}`;
      return apiFetch<AiDecisionsView>(`/v2/ai-decisions${qp}`);
    },
    refetchInterval: 10_000,
  });

  const decide = useMutation({
    mutationFn: ({ id, status, admin_note }: DecideArgs) =>
      apiFetch<{ updated: number }>(`/ai/approval-requests/${id}`, {
        method: "PATCH",
        body: JSON.stringify({ status, admin_note }),
      }),
    onSuccess: () => {
      setError(null);
      qc.invalidateQueries({ queryKey: ["v2", "ai-decisions"] });
    },
    onError: (e) => setError((e as Error).message),
  });

  const handleDecision = (id: string, decision: "approved" | "rejected") => {
    const note = decision === "rejected" ? window.prompt("Reject note (optional):") ?? undefined : undefined;
    decide.mutate({ id, status: decision, admin_note: note });
  };

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap gap-2">
        {FILTERS.map((f) => (
          <button
            key={f.value}
            type="button"
            onClick={() => setStatus(f.value)}
            className={`rounded border px-3 py-1 text-xs uppercase ${
              status === f.value
                ? "border-emerald-500/40 bg-emerald-500/15 text-emerald-300"
                : "border-zinc-700 text-zinc-400 hover:border-zinc-500 hover:text-zinc-200"
            }`}
          >
            {f.label}
          </button>
        ))}
      </div>

      {error && (
        <div className="rounded border border-red-800 bg-red-950/40 px-3 py-2 text-xs text-red-300">
          {error}
        </div>
      )}

      {query.isLoading && <div className="text-sm text-zinc-400">Loading…</div>}
      {query.isError && (
        <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
          Failed: {(query.error as Error).message}
        </div>
      )}
      {query.data && (
        <div className="overflow-hidden rounded-lg border border-zinc-800 bg-zinc-900/60">
          {query.data.entries.length === 0 ? (
            <div className="px-4 py-6 text-sm text-zinc-500">No decisions match this filter.</div>
          ) : (
            <table className="w-full text-sm">
              <thead className="bg-zinc-900/80 text-xs uppercase text-zinc-500">
                <tr>
                  <th className="px-3 py-2 text-left">Status</th>
                  <th className="px-3 py-2 text-left">Kind</th>
                  <th className="px-3 py-2 text-left">Model</th>
                  <th className="px-3 py-2 text-left">Payload</th>
                  <th className="px-3 py-2 text-left">Created</th>
                  <th className="px-3 py-2 text-left">Decided</th>
                  <th className="px-3 py-2 text-left">Actions</th>
                </tr>
              </thead>
              <tbody>
                {query.data.entries.map((e) => (
                  <tr key={e.id} className="border-t border-zinc-800/60 align-top">
                    <td className="px-3 py-2">
                      <span
                        className={`inline-block rounded border px-2 py-0.5 text-xs uppercase ${STATUS_BADGE[e.status]}`}
                      >
                        {e.status}
                      </span>
                    </td>
                    <td className="px-3 py-2 font-mono text-zinc-200">{e.kind}</td>
                    <td className="px-3 py-2 text-xs text-zinc-400">{e.model_hint ?? "—"}</td>
                    <td className="px-3 py-2 font-mono text-xs text-zinc-300">
                      {e.payload_preview}
                      {e.admin_note && (
                        <div className="mt-1 text-zinc-500">note: {e.admin_note}</div>
                      )}
                    </td>
                    <td className="px-3 py-2 font-mono text-xs text-zinc-500">{e.created_at}</td>
                    <td className="px-3 py-2 font-mono text-xs text-zinc-500">
                      {e.decided_at ?? "—"}
                    </td>
                    <td className="px-3 py-2">
                      {e.status === "pending" ? (
                        <div className="flex gap-1">
                          <button
                            type="button"
                            disabled={decide.isPending}
                            onClick={() => handleDecision(e.id, "approved")}
                            className="rounded border border-emerald-500/40 bg-emerald-500/10 px-2 py-1 text-xs text-emerald-300 hover:bg-emerald-500/20 disabled:opacity-50"
                          >
                            Approve
                          </button>
                          <button
                            type="button"
                            disabled={decide.isPending}
                            onClick={() => handleDecision(e.id, "rejected")}
                            className="rounded border border-red-500/40 bg-red-500/10 px-2 py-1 text-xs text-red-300 hover:bg-red-500/20 disabled:opacity-50"
                          >
                            Reject
                          </button>
                        </div>
                      ) : (
                        <span className="text-xs text-zinc-600">—</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}
    </div>
  );
}
