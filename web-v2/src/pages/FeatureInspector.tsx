import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type {
  SourceCoverage,
  FeatureStat,
  FeatureSnapshotRow,
} from "../lib/types";

// ── Helpers ──────────────────────────────────────────────────────────

const HOUR_OPTIONS = [6, 12, 24, 48] as const;

function fmtTs(iso: string | null): string {
  if (!iso) return "\u2014";
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

function fmtNum(v: number | null | undefined, digits = 4): string {
  if (v === null || v === undefined) return "\u2014";
  return v.toFixed(digits);
}

function staleness(lastAt: string | null): "green" | "amber" | "red" {
  if (!lastAt) return "red";
  const diffMs = Date.now() - new Date(lastAt).getTime();
  if (diffMs < 5 * 60_000) return "green";
  if (diffMs < 30 * 60_000) return "amber";
  return "red";
}

const STALE_BORDER: Record<string, string> = {
  green: "border-emerald-500/50",
  amber: "border-amber-500/50",
  red: "border-red-500/50",
};

const STALE_DOT: Record<string, string> = {
  green: "bg-emerald-400",
  amber: "bg-amber-400",
  red: "bg-red-400",
};

// ── Source Coverage Card ─────────────────────────────────────────────

function CoverageCard({
  cov,
  selected,
  onClick,
}: {
  cov: SourceCoverage;
  selected: boolean;
  onClick: () => void;
}) {
  const freshness = staleness(cov.last_at);
  return (
    <button
      onClick={onClick}
      className={`rounded-lg border ${STALE_BORDER[freshness]} bg-zinc-900/60 px-4 py-3 text-left transition hover:bg-zinc-800/60 ${
        selected ? "ring-2 ring-emerald-500/40" : ""
      }`}
    >
      <div className="flex items-center gap-2">
        <span className={`h-2 w-2 rounded-full ${STALE_DOT[freshness]}`} />
        <span className="text-sm font-semibold text-zinc-100">
          {cov.source}
        </span>
      </div>
      <div className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1 text-[11px] text-zinc-400">
        <span>Snapshots</span>
        <span className="text-zinc-200">{cov.n_snapshots}</span>
        <span>Features</span>
        <span className="text-zinc-200">{cov.n_features}</span>
        <span>Spec</span>
        <span className="text-zinc-200">{cov.spec_version}</span>
        <span>Last update</span>
        <span className="text-zinc-200">{fmtTs(cov.last_at)}</span>
      </div>
    </button>
  );
}

// ── Feature Stats Table ─────────────────────────────────────────────

function StatsTable({ stats, maxN }: { stats: FeatureStat[]; maxN: number }) {
  return (
    <div className="overflow-x-auto rounded-lg border border-zinc-800">
      <table className="w-full text-left text-[11px]">
        <thead className="border-b border-zinc-800 bg-zinc-900/80 text-zinc-400">
          <tr>
            <th className="px-3 py-2">Feature</th>
            <th className="px-3 py-2 text-right">Count</th>
            <th className="px-3 py-2 text-right">Mean</th>
            <th className="px-3 py-2 text-right">Min</th>
            <th className="px-3 py-2 text-right">Max</th>
            <th className="px-3 py-2 text-right">StdDev</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-zinc-800/50">
          {stats.map((s) => {
            const lowCount = maxN > 0 && s.n < maxN * 0.5;
            const highStd =
              s.stddev !== null &&
              s.mean !== null &&
              s.mean !== 0 &&
              Math.abs(s.stddev / s.mean) > 2;
            const rowCls = lowCount
              ? "bg-amber-500/5"
              : highStd
                ? "bg-red-500/5"
                : "";
            return (
              <tr key={s.feature} className={rowCls}>
                <td className="px-3 py-1.5 font-mono text-zinc-200">
                  {s.feature}
                  {lowCount && (
                    <span className="ml-1 text-[9px] text-amber-400">GAP</span>
                  )}
                </td>
                <td className="px-3 py-1.5 text-right text-zinc-300">
                  {s.n}
                </td>
                <td className="px-3 py-1.5 text-right font-mono text-zinc-300">
                  {fmtNum(s.mean)}
                </td>
                <td className="px-3 py-1.5 text-right font-mono text-zinc-300">
                  {fmtNum(s.min_val)}
                </td>
                <td className="px-3 py-1.5 text-right font-mono text-zinc-300">
                  {fmtNum(s.max_val)}
                </td>
                <td className="px-3 py-1.5 text-right font-mono text-zinc-300">
                  {fmtNum(s.stddev)}
                  {highStd && (
                    <span className="ml-1 text-[9px] text-red-400">HIGH</span>
                  )}
                </td>
              </tr>
            );
          })}
          {stats.length === 0 && (
            <tr>
              <td colSpan={6} className="px-3 py-4 text-center text-zinc-500">
                No stats available. Select a source above.
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}

// ── Snapshot Expansion ──────────────────────────────────────────────

function SnapshotDetail({
  features,
}: {
  features: Record<string, number | string | null>;
}) {
  const keys = Object.keys(features).sort();
  return (
    <div className="grid grid-cols-2 gap-x-6 gap-y-0.5 px-6 py-2 text-[10px] sm:grid-cols-3 md:grid-cols-4">
      {keys.map((k) => {
        const v = features[k];
        const isNull = v === null || v === undefined;
        return (
          <div key={k} className="flex justify-between gap-2">
            <span className="truncate text-zinc-400">{k}</span>
            <span
              className={`font-mono ${isNull ? "text-red-400" : "text-zinc-200"}`}
            >
              {isNull ? "null" : String(v)}
            </span>
          </div>
        );
      })}
    </div>
  );
}

// ── Main Page ───────────────────────────────────────────────────────

export function FeatureInspector() {
  const [hours, setHours] = useState<number>(24);
  const [selectedSource, setSelectedSource] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const coverageQ = useQuery({
    queryKey: ["feature-coverage", hours],
    queryFn: () =>
      apiFetch<{ sources: SourceCoverage[] }>(
        `/v2/ml/features/coverage?hours=${hours}`,
      ).then((r) => r.sources),
    refetchInterval: 30_000,
  });

  const statsQ = useQuery({
    queryKey: ["feature-stats", selectedSource, hours],
    queryFn: () =>
      apiFetch<{ stats: FeatureStat[] }>(
        `/v2/ml/features/stats?source=${selectedSource}&hours=${hours}`,
      ).then((r) => r.stats),
    enabled: !!selectedSource,
  });

  const snapshotsQ = useQuery({
    queryKey: ["feature-snapshots", selectedSource],
    queryFn: () =>
      apiFetch<{ snapshots: FeatureSnapshotRow[] }>(
        `/v2/ml/features/snapshots?limit=20${selectedSource ? `&source=${selectedSource}` : ""}`,
      ).then((r) => r.snapshots),
    refetchInterval: 30_000,
  });

  const maxN =
    statsQ.data && statsQ.data.length > 0
      ? Math.max(...statsQ.data.map((s) => s.n))
      : 0;

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold text-zinc-100">
          Feature Inspector
        </h1>
        <div className="flex gap-1">
          {HOUR_OPTIONS.map((h) => (
            <button
              key={h}
              onClick={() => setHours(h)}
              className={`rounded px-2.5 py-1 text-xs transition ${
                hours === h
                  ? "bg-emerald-500/20 text-emerald-300"
                  : "bg-zinc-800 text-zinc-400 hover:text-zinc-200"
              }`}
            >
              {h}h
            </button>
          ))}
        </div>
      </div>

      {/* Source coverage cards */}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
        {coverageQ.data?.map((cov) => (
          <CoverageCard
            key={`${cov.source}-${cov.spec_version}`}
            cov={cov}
            selected={selectedSource === cov.source}
            onClick={() =>
              setSelectedSource((prev) =>
                prev === cov.source ? null : cov.source,
              )
            }
          />
        ))}
        {coverageQ.isLoading && (
          <p className="text-sm text-zinc-500">Loading coverage...</p>
        )}
        {coverageQ.data?.length === 0 && (
          <p className="text-sm text-zinc-500">
            No feature snapshots in the last {hours}h.
          </p>
        )}
      </div>

      {/* Feature statistics table */}
      {selectedSource && (
        <div>
          <h2 className="mb-2 text-sm font-medium text-zinc-300">
            Feature Statistics{" "}
            <span className="text-zinc-500">({selectedSource})</span>
          </h2>
          {statsQ.isLoading ? (
            <p className="text-sm text-zinc-500">Loading stats...</p>
          ) : (
            <StatsTable stats={statsQ.data ?? []} maxN={maxN} />
          )}
        </div>
      )}

      {/* Recent snapshots */}
      <div>
        <h2 className="mb-2 text-sm font-medium text-zinc-300">
          Recent Snapshots
          {selectedSource && (
            <span className="text-zinc-500"> ({selectedSource})</span>
          )}
        </h2>
        <div className="overflow-x-auto rounded-lg border border-zinc-800">
          <table className="w-full text-left text-[11px]">
            <thead className="border-b border-zinc-800 bg-zinc-900/80 text-zinc-400">
              <tr>
                <th className="px-3 py-2">Time</th>
                <th className="px-3 py-2">Source</th>
                <th className="px-3 py-2">Spec</th>
                <th className="px-3 py-2">Detection</th>
                <th className="px-3 py-2 text-right">Keys</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-800/50">
              {snapshotsQ.data?.map((snap) => {
                const nKeys = Object.keys(snap.features_json).length;
                const isExpanded = expandedId === snap.id;
                return (
                  <tr key={snap.id} className="group">
                    <td colSpan={5} className="p-0">
                      <button
                        className="flex w-full items-center text-left hover:bg-zinc-800/40"
                        onClick={() =>
                          setExpandedId(isExpanded ? null : snap.id)
                        }
                      >
                        <span className="w-1/4 px-3 py-1.5 text-zinc-300">
                          {fmtTs(snap.created_at)}
                        </span>
                        <span className="w-1/6 px-3 py-1.5 text-zinc-200">
                          {snap.source}
                        </span>
                        <span className="w-1/6 px-3 py-1.5 text-zinc-400">
                          {snap.feature_spec_version}
                        </span>
                        <span className="w-1/4 px-3 py-1.5 font-mono text-zinc-500">
                          {snap.detection_id?.slice(0, 8) ?? "\u2014"}
                        </span>
                        <span className="w-1/12 px-3 py-1.5 text-right text-zinc-300">
                          {nKeys}
                        </span>
                      </button>
                      {isExpanded && (
                        <SnapshotDetail features={snap.features_json} />
                      )}
                    </td>
                  </tr>
                );
              })}
              {snapshotsQ.data?.length === 0 && (
                <tr>
                  <td
                    colSpan={5}
                    className="px-3 py-4 text-center text-zinc-500"
                  >
                    No snapshots found.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
