import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";

// Faz 9.3 — Model registry viewer.
// Lists rows from qtss_models so operators can:
//   * compare AUC / logloss across training runs,
//   * flip which model is active for a family (served by the future
//     Rust inference hook),
//   * audit who trained what and when.

type ModelEntry = {
  id: string;
  model_family: string;
  model_version: string;
  feature_spec_version: number;
  algorithm: string;
  task: string;
  n_train: number;
  n_valid: number;
  metrics: Record<string, number | null>;
  feature_count: number;
  artifact_path: string;
  artifact_sha256: string | null;
  trained_at: string;
  trained_by: string | null;
  notes: string | null;
  active: boolean;
};

type ModelList = {
  generated_at: string;
  entries: ModelEntry[];
};

function fmtTs(iso: string | null): string {
  if (!iso) return "—";
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

function fmtMetric(v: number | null | undefined, digits = 4): string {
  if (v == null || Number.isNaN(v)) return "—";
  return Number(v).toFixed(digits);
}

export function Models() {
  const qc = useQueryClient();
  const [familyFilter, setFamilyFilter] = useState<string>("");

  const q = useQuery({
    queryKey: ["v2", "models", { family: familyFilter }],
    queryFn: () => {
      const p = new URLSearchParams();
      if (familyFilter) p.set("family", familyFilter);
      return apiFetch<ModelList>(
        `/v2/models${p.toString() ? `?${p.toString()}` : ""}`,
      );
    },
    refetchInterval: 15_000,
  });

  const activateMut = useMutation({
    mutationFn: (vars: { family: string; version: string }) =>
      apiFetch<{ ok: boolean }>("/v2/models/activate", {
        method: "POST",
        body: JSON.stringify(vars),
      }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["v2", "models"] }),
  });

  if (q.isLoading) {
    return <div className="text-sm text-zinc-400">Loading models…</div>;
  }
  if (q.isError) {
    return (
      <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
        Failed to load: {(q.error as Error).message}
      </div>
    );
  }
  const entries = q.data?.entries ?? [];
  const families = Array.from(
    new Set(entries.map((e) => e.model_family)),
  ).sort();

  return (
    <div className="space-y-4">
      <div className="flex items-baseline justify-between">
        <div className="text-xs uppercase tracking-wide text-zinc-500">
          Model Registry — {entries.length} row{entries.length === 1 ? "" : "s"}
        </div>
        <div className="text-xs text-zinc-500">
          {q.data ? `Generated at ${fmtTs(q.data.generated_at)}` : ""}
        </div>
      </div>

      <div className="flex flex-wrap gap-2 text-xs">
        <select
          value={familyFilter}
          onChange={(e) => setFamilyFilter(e.target.value)}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          <option value="">all families</option>
          {families.map((f) => (
            <option key={f} value={f}>
              {f}
            </option>
          ))}
        </select>
      </div>

      {entries.length === 0 ? (
        <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 px-4 py-6 text-sm text-zinc-500">
          No trained models yet. Run{" "}
          <code className="rounded bg-zinc-800 px-1 py-0.5">
            qtss-trainer train
          </code>{" "}
          on the worker host once the training set is ready.
        </div>
      ) : (
        <div className="overflow-x-auto rounded-lg border border-zinc-800 bg-zinc-900/40">
          <table className="min-w-full">
            <thead className="bg-zinc-900/80 text-[10px] uppercase tracking-wide text-zinc-500">
              <tr>
                <th className="px-2 py-2 text-left">Active</th>
                <th className="px-2 py-2 text-left">Family</th>
                <th className="px-2 py-2 text-left">Version</th>
                <th className="px-2 py-2 text-left">Spec v</th>
                <th className="px-2 py-2 text-right">Rows</th>
                <th className="px-2 py-2 text-right">Feats</th>
                <th className="px-2 py-2 text-right">AUC</th>
                <th className="px-2 py-2 text-right">Logloss</th>
                <th className="px-2 py-2 text-right">PR-AUC</th>
                <th className="px-2 py-2 text-left">Trained</th>
                <th className="px-2 py-2 text-left">By</th>
                <th className="px-2 py-2 text-left"></th>
              </tr>
            </thead>
            <tbody>
              {entries.map((m) => (
                <tr
                  key={m.id}
                  className="border-b border-zinc-800/60 text-xs hover:bg-zinc-800/30"
                >
                  <td className="px-2 py-1.5">
                    {m.active ? (
                      <span className="rounded border border-emerald-500/40 bg-emerald-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-emerald-300">
                        active
                      </span>
                    ) : (
                      <span className="text-zinc-600">—</span>
                    )}
                  </td>
                  <td className="px-2 py-1.5 text-zinc-300">
                    {m.model_family}
                  </td>
                  <td className="px-2 py-1.5 font-mono text-zinc-200">
                    {m.model_version}
                  </td>
                  <td className="px-2 py-1.5 text-center text-zinc-400">
                    {m.feature_spec_version}
                  </td>
                  <td className="px-2 py-1.5 text-right font-mono text-zinc-400">
                    {m.n_train}/{m.n_valid}
                  </td>
                  <td className="px-2 py-1.5 text-right font-mono text-zinc-400">
                    {m.feature_count}
                  </td>
                  <td className="px-2 py-1.5 text-right font-mono text-emerald-300">
                    {fmtMetric(m.metrics.auc)}
                  </td>
                  <td className="px-2 py-1.5 text-right font-mono text-zinc-300">
                    {fmtMetric(m.metrics.logloss)}
                  </td>
                  <td className="px-2 py-1.5 text-right font-mono text-zinc-300">
                    {fmtMetric(m.metrics.pr_auc)}
                  </td>
                  <td className="px-2 py-1.5 font-mono text-zinc-500">
                    {fmtTs(m.trained_at)}
                  </td>
                  <td className="px-2 py-1.5 text-zinc-500">
                    {m.trained_by ?? "—"}
                  </td>
                  <td className="px-2 py-1.5">
                    {!m.active && (
                      <button
                        type="button"
                        disabled={activateMut.isPending}
                        onClick={() =>
                          activateMut.mutate({
                            family: m.model_family,
                            version: m.model_version,
                          })
                        }
                        className="rounded border border-emerald-700 bg-emerald-900/30 px-2 py-0.5 text-[10px] text-emerald-300 hover:bg-emerald-900/60 disabled:opacity-50"
                      >
                        activate
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
