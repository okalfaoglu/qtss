import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";

// Faz 9.2.1/9.2.2 — Training Set Monitor.
// Tells the operator whether the Faz 9.3 LightGBM trainer has enough
// labeled data + feature coverage to kick off. Thresholds are resolved
// server-side from config_schema (setup.training_set.*).

type LabelBucket = { label: string; n: number };
type FeatureCoverage = { source: string; n: number };

type TrainingSetStats = {
  total_setups: number;
  closed_setups: number;
  labeled_setups: number;
  setups_with_features: number;
  label_distribution: LabelBucket[];
  feature_coverage: FeatureCoverage[];
};

type Readiness = {
  min_closed: number;
  min_feature_coverage_pct: number;
  closed_ok: boolean;
  features_ok: boolean;
  ready: boolean;
};

type Payload = {
  generated_at: string;
  stats: TrainingSetStats;
  readiness: Readiness;
};

function fmtTs(iso: string | null): string {
  if (!iso) return "—";
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

function Pill({
  ok,
  children,
}: {
  ok: boolean;
  children: React.ReactNode;
}) {
  return (
    <span
      className={`rounded border px-2 py-0.5 text-[11px] ${
        ok
          ? "border-emerald-500/40 bg-emerald-500/15 text-emerald-300"
          : "border-amber-500/40 bg-amber-500/15 text-amber-300"
      }`}
    >
      {children}
    </span>
  );
}

export function TrainingSet() {
  const query = useQuery({
    queryKey: ["v2", "training-set", "stats"],
    queryFn: () => apiFetch<Payload>("/v2/training-set/stats"),
    refetchInterval: 30_000,
  });

  if (query.isLoading) {
    return <div className="text-sm text-zinc-400">Loading training set…</div>;
  }
  if (query.isError) {
    return (
      <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
        Failed to load: {(query.error as Error).message}
      </div>
    );
  }
  const data = query.data!;
  const s = data.stats;
  const r = data.readiness;
  const coveragePct =
    s.total_setups > 0 ? (s.setups_with_features / s.total_setups) * 100 : 0;

  return (
    <div className="space-y-4">
      <div className="flex items-baseline justify-between">
        <div className="text-xs uppercase tracking-wide text-zinc-500">
          Training Set Monitor — Faz 9.3 readiness
        </div>
        <div className="text-xs text-zinc-500">
          Generated at {fmtTs(data.generated_at)}
        </div>
      </div>

      {/* Readiness banner */}
      <div
        className={`rounded-lg border p-4 ${
          r.ready
            ? "border-emerald-600/40 bg-emerald-900/20"
            : "border-amber-600/40 bg-amber-900/20"
        }`}
      >
        <div className="flex items-baseline justify-between">
          <div className="text-sm font-semibold text-zinc-100">
            {r.ready
              ? "Ready — trainer can spin up."
              : "Not ready — accumulating data."}
          </div>
          <div className="flex gap-2">
            <Pill ok={r.closed_ok}>
              closed {s.closed_setups}/{r.min_closed}
            </Pill>
            <Pill ok={r.features_ok}>
              coverage {coveragePct.toFixed(1)}% / {r.min_feature_coverage_pct}%
            </Pill>
          </div>
        </div>
      </div>

      {/* Top-line counters */}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        {[
          { label: "Total setups", value: s.total_setups },
          { label: "Closed", value: s.closed_setups },
          { label: "Labeled", value: s.labeled_setups },
          { label: "With features", value: s.setups_with_features },
        ].map((c) => (
          <div
            key={c.label}
            className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3"
          >
            <div className="text-[10px] uppercase tracking-wide text-zinc-500">
              {c.label}
            </div>
            <div className="mt-1 font-mono text-xl text-zinc-100">
              {c.value}
            </div>
          </div>
        ))}
      </div>

      {/* Label distribution */}
      <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
        <div className="mb-2 text-[10px] uppercase tracking-wide text-zinc-500">
          Label distribution
        </div>
        {s.label_distribution.length === 0 ? (
          <div className="text-xs text-zinc-500">No setups yet.</div>
        ) : (
          <div className="flex flex-wrap gap-1.5">
            {s.label_distribution.map((b) => (
              <span
                key={b.label}
                className="rounded border border-zinc-700 bg-zinc-800/60 px-2 py-1 text-[11px] text-zinc-300"
              >
                <span className="font-mono">{b.label}</span>
                <span className="ml-1.5 font-mono font-semibold text-zinc-100">
                  {b.n}
                </span>
              </span>
            ))}
          </div>
        )}
      </div>

      {/* Feature coverage per source */}
      <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
        <div className="mb-2 text-[10px] uppercase tracking-wide text-zinc-500">
          Feature coverage by source
        </div>
        {s.feature_coverage.length === 0 ? (
          <div className="text-xs text-zinc-500">
            No feature snapshots yet — detection pipeline has not written any
            rows to <code>qtss_features_snapshot</code>.
          </div>
        ) : (
          <div className="flex flex-wrap gap-1.5">
            {s.feature_coverage.map((f) => (
              <span
                key={f.source}
                className="rounded border border-sky-500/30 bg-sky-500/15 px-2 py-1 text-[11px] text-sky-300"
              >
                <span className="font-mono">{f.source}</span>
                <span className="ml-1.5 font-mono font-semibold">{f.n}</span>
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
