import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";

// Faz 9B — Drift dashboard.
// Wraps three tables from migration 0169:
//   * qtss_ml_drift_snapshots  (PSI per feature, updated by psi_drift_loop)
//   * qtss_ml_breaker_events   (circuit breaker trips)
//   * qtss_models              (joined for model_version → event label)
// Operator workflow (runbook FAZ_9B_DRIFT_RUNBOOK §2):
//   1. Glance at the feature table — worst PSI at the top.
//   2. Click a feature row → sparkline timeline for the last week.
//   3. If a breaker fired, enter resolved_by + note and close it
//      from the breaker card (POST /v2/drift/breakers/:id/resolve).

type DriftBands = { warn: number; critical: number };

type DriftFeature = {
  feature_name: string;
  model_version: string;
  psi: number;
  status: "ok" | "warn" | "critical";
  computed_at: string;
};

type DriftSnapshots = {
  generated_at: string;
  bands: DriftBands;
  features: DriftFeature[];
};

type DriftTimelinePoint = { psi: number; computed_at: string };

type DriftTimeline = {
  feature_name: string;
  bands: DriftBands;
  points: DriftTimelinePoint[];
};

type BreakerEvent = {
  id: string;
  fired_at: string;
  model_id: string;
  model_version: string | null;
  action: string;
  reason: string;
  critical_features: Array<{ feature: string; psi: number }> | unknown;
  resolved_at: string | null;
  resolved_by: string | null;
  resolution_note: string | null;
};

type BreakerList = {
  generated_at: string;
  events: BreakerEvent[];
};

type CalibrationBin = {
  lo: number;
  hi: number;
  n: number;
  mean_predicted: number;
  realized_win_rate: number;
  gap: number;
};

type CalibrationReport = {
  generated_at: string;
  model_version: string | null;
  days: number;
  n_total: number;
  n_positive: number;
  brier: number;
  bins: CalibrationBin[];
};

// Status → Tailwind class. Keeps the JSX shallow (CLAUDE.md #1).
const STATUS_PILL: Record<string, string> = {
  ok: "border-emerald-500/40 bg-emerald-500/10 text-emerald-300",
  warn: "border-amber-500/40 bg-amber-500/10 text-amber-300",
  critical: "border-red-500/50 bg-red-500/15 text-red-300",
};

function fmtTs(iso: string | null): string {
  if (!iso) return "—";
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

function fmtPsi(v: number): string {
  return v.toFixed(3);
}

// Minimal inline SVG sparkline — no chart lib, avoids a new dep. Scales
// y to [0, max(psi, critical)] so the critical band always shows even
// when the series is tame.
function Sparkline({
  points,
  bands,
}: {
  points: DriftTimelinePoint[];
  bands: DriftBands;
}) {
  if (points.length < 2) {
    return (
      <div className="flex h-16 items-center justify-center rounded border border-zinc-800 bg-zinc-950 text-[10px] text-zinc-600">
        Not enough data (needs ≥2 snapshots)
      </div>
    );
  }
  const w = 480;
  const h = 64;
  const max = Math.max(bands.critical * 1.1, ...points.map((p) => p.psi), 0.01);
  const x = (i: number) => (i / (points.length - 1)) * w;
  const y = (v: number) => h - (v / max) * h;
  const path = points
    .map((p, i) => `${i === 0 ? "M" : "L"}${x(i).toFixed(1)},${y(p.psi).toFixed(1)}`)
    .join(" ");
  return (
    <svg
      viewBox={`0 0 ${w} ${h}`}
      className="h-16 w-full rounded border border-zinc-800 bg-zinc-950"
      preserveAspectRatio="none"
    >
      {/* critical band */}
      <rect
        x={0}
        y={0}
        width={w}
        height={y(bands.critical)}
        fill="rgba(248,113,113,0.08)"
      />
      {/* warn band */}
      <rect
        x={0}
        y={y(bands.critical)}
        width={w}
        height={y(bands.warn) - y(bands.critical)}
        fill="rgba(251,191,36,0.08)"
      />
      {/* threshold lines */}
      <line
        x1={0}
        x2={w}
        y1={y(bands.critical)}
        y2={y(bands.critical)}
        stroke="rgba(248,113,113,0.5)"
        strokeDasharray="3 3"
      />
      <line
        x1={0}
        x2={w}
        y1={y(bands.warn)}
        y2={y(bands.warn)}
        stroke="rgba(251,191,36,0.5)"
        strokeDasharray="3 3"
      />
      <path d={path} fill="none" stroke="#60a5fa" strokeWidth={1.5} />
    </svg>
  );
}

export function Drift() {
  const qc = useQueryClient();
  const [selected, setSelected] = useState<string | null>(null);
  const [resolveId, setResolveId] = useState<string | null>(null);
  const [resolvedBy, setResolvedBy] = useState("");
  const [resolutionNote, setResolutionNote] = useState("");

  const snapshots = useQuery({
    queryKey: ["v2", "drift", "snapshots"],
    queryFn: () => apiFetch<DriftSnapshots>("/v2/drift/snapshots"),
    refetchInterval: 30_000,
  });

  const timeline = useQuery({
    enabled: selected != null,
    queryKey: ["v2", "drift", "timeline", selected],
    queryFn: () =>
      apiFetch<DriftTimeline>(
        `/v2/drift/timeline?feature=${encodeURIComponent(selected!)}&hours=168`,
      ),
    refetchInterval: 60_000,
  });

  const breakers = useQuery({
    queryKey: ["v2", "drift", "breakers"],
    queryFn: () => apiFetch<BreakerList>("/v2/drift/breakers?limit=50"),
    refetchInterval: 30_000,
  });

  // Faz 9B Kalem G — calibration monitor. Pulls a 30-day window by
  // default; operator tunes via trainer runs, not through the GUI.
  const calibration = useQuery({
    queryKey: ["v2", "drift", "calibration"],
    queryFn: () =>
      apiFetch<CalibrationReport>("/v2/drift/calibration?days=30"),
    refetchInterval: 60_000,
  });

  const resolveMut = useMutation({
    mutationFn: (vars: {
      id: string;
      resolved_by: string;
      resolution_note: string;
    }) =>
      apiFetch<{ ok: boolean }>(`/v2/drift/breakers/${vars.id}/resolve`, {
        method: "POST",
        body: JSON.stringify({
          resolved_by: vars.resolved_by,
          resolution_note: vars.resolution_note || null,
        }),
      }),
    onSuccess: () => {
      setResolveId(null);
      setResolvedBy("");
      setResolutionNote("");
      qc.invalidateQueries({ queryKey: ["v2", "drift", "breakers"] });
    },
  });

  const openBreakers = useMemo(
    () => (breakers.data?.events ?? []).filter((e) => e.resolved_at == null),
    [breakers.data],
  );
  const resolvedBreakers = useMemo(
    () => (breakers.data?.events ?? []).filter((e) => e.resolved_at != null),
    [breakers.data],
  );

  return (
    <div className="space-y-6">
      {/* Header + band legend */}
      <div className="flex items-baseline justify-between">
        <div className="text-xs uppercase tracking-wide text-zinc-500">
          PSI Drift Dashboard — {snapshots.data?.features.length ?? 0} features
        </div>
        <div className="text-xs text-zinc-500">
          {snapshots.data
            ? `generated ${fmtTs(snapshots.data.generated_at)} · warn ≥ ${
                snapshots.data.bands.warn
              } · critical ≥ ${snapshots.data.bands.critical}`
            : ""}
        </div>
      </div>

      {/* Open breakers banner */}
      {openBreakers.length > 0 && (
        <div className="rounded-lg border border-red-700 bg-red-950/40 p-4">
          <div className="text-sm font-semibold text-red-300">
            {openBreakers.length} unresolved breaker event
            {openBreakers.length === 1 ? "" : "s"}
          </div>
          <div className="mt-2 space-y-3">
            {openBreakers.map((e) => (
              <div
                key={e.id}
                className="rounded border border-red-800/60 bg-red-950/30 p-3 text-xs text-zinc-200"
              >
                <div className="flex items-baseline justify-between">
                  <div className="font-mono text-red-200">
                    {fmtTs(e.fired_at)} · {e.action}
                  </div>
                  <div className="font-mono text-zinc-500">
                    {e.model_version ?? "(unknown model)"}
                  </div>
                </div>
                <div className="mt-1 text-zinc-300">{e.reason}</div>
                {resolveId === e.id ? (
                  <div className="mt-3 space-y-2 rounded border border-zinc-700 bg-zinc-900/60 p-2">
                    <input
                      type="text"
                      value={resolvedBy}
                      placeholder="resolved_by (required, e.g. oguz)"
                      onChange={(ev) => setResolvedBy(ev.target.value)}
                      className="w-full rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-xs text-zinc-200"
                    />
                    <textarea
                      value={resolutionNote}
                      placeholder="resolution note (retrained / root cause / follow-up)"
                      onChange={(ev) => setResolutionNote(ev.target.value)}
                      rows={2}
                      className="w-full rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-xs text-zinc-200"
                    />
                    <div className="flex gap-2">
                      <button
                        type="button"
                        disabled={
                          resolveMut.isPending || resolvedBy.trim() === ""
                        }
                        onClick={() =>
                          resolveMut.mutate({
                            id: e.id,
                            resolved_by: resolvedBy.trim(),
                            resolution_note: resolutionNote.trim(),
                          })
                        }
                        className="rounded border border-emerald-700 bg-emerald-900/40 px-2 py-1 text-[10px] text-emerald-300 hover:bg-emerald-900/70 disabled:opacity-50"
                      >
                        confirm resolve
                      </button>
                      <button
                        type="button"
                        onClick={() => {
                          setResolveId(null);
                          setResolvedBy("");
                          setResolutionNote("");
                        }}
                        className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-[10px] text-zinc-300 hover:bg-zinc-800"
                      >
                        cancel
                      </button>
                    </div>
                  </div>
                ) : (
                  <button
                    type="button"
                    onClick={() => setResolveId(e.id)}
                    className="mt-2 rounded border border-emerald-700 bg-emerald-900/30 px-2 py-0.5 text-[10px] text-emerald-300 hover:bg-emerald-900/60"
                  >
                    mark resolved
                  </button>
                )}
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Feature table + selected timeline */}
      <div className="grid gap-4 lg:grid-cols-[1fr_2fr]">
        <div className="overflow-x-auto rounded-lg border border-zinc-800 bg-zinc-900/40">
          <table className="min-w-full">
            <thead className="bg-zinc-900/80 text-[10px] uppercase tracking-wide text-zinc-500">
              <tr>
                <th className="px-2 py-2 text-left">Status</th>
                <th className="px-2 py-2 text-left">Feature</th>
                <th className="px-2 py-2 text-right">PSI</th>
                <th className="px-2 py-2 text-left">Snapshot</th>
              </tr>
            </thead>
            <tbody>
              {(snapshots.data?.features ?? []).map((f) => (
                <tr
                  key={f.feature_name}
                  onClick={() => setSelected(f.feature_name)}
                  className={`cursor-pointer border-b border-zinc-800/60 text-xs hover:bg-zinc-800/40 ${
                    selected === f.feature_name ? "bg-zinc-800/60" : ""
                  }`}
                >
                  <td className="px-2 py-1.5">
                    <span
                      className={`rounded border px-1.5 py-0.5 text-[10px] font-semibold ${
                        STATUS_PILL[f.status] ?? STATUS_PILL.ok
                      }`}
                    >
                      {f.status}
                    </span>
                  </td>
                  <td className="px-2 py-1.5 font-mono text-zinc-200">
                    {f.feature_name}
                  </td>
                  <td className="px-2 py-1.5 text-right font-mono text-zinc-300">
                    {fmtPsi(f.psi)}
                  </td>
                  <td className="px-2 py-1.5 font-mono text-zinc-500">
                    {fmtTs(f.computed_at)}
                  </td>
                </tr>
              ))}
              {snapshots.data?.features.length === 0 && (
                <tr>
                  <td
                    colSpan={4}
                    className="px-2 py-6 text-center text-xs text-zinc-500"
                  >
                    No drift snapshots yet. Sidecar PSI loop writes on its
                    next tick once a trained model is active.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        <div className="space-y-2 rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
          <div className="text-xs uppercase tracking-wide text-zinc-500">
            {selected
              ? `Timeline · ${selected} (last 7d)`
              : "Select a feature to view PSI history"}
          </div>
          {selected && timeline.data && (
            <Sparkline
              points={timeline.data.points}
              bands={timeline.data.bands}
            />
          )}
          {selected && timeline.data && (
            <div className="text-[10px] text-zinc-500">
              {timeline.data.points.length} snapshots · worst{" "}
              {fmtPsi(
                timeline.data.points.reduce(
                  (m, p) => Math.max(m, p.psi),
                  0,
                ),
              )}
            </div>
          )}
        </div>
      </div>

      {/* Calibration monitor (Kalem G) */}
      {calibration.data && (
        <div className="space-y-2">
          <div className="flex items-baseline justify-between">
            <div className="text-xs uppercase tracking-wide text-zinc-500">
              Calibration · last {calibration.data.days}d
            </div>
            <div className="text-xs text-zinc-500">
              {calibration.data.n_total} closed setups · win rate{" "}
              {calibration.data.n_total > 0
                ? (
                    (calibration.data.n_positive / calibration.data.n_total) *
                    100
                  ).toFixed(1)
                : "—"}
              % · Brier {calibration.data.brier.toFixed(4)}
            </div>
          </div>
          <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
            {calibration.data.n_total === 0 ? (
              <div className="text-xs text-zinc-500">
                No closed setups with linked predictions yet. Calibration
                fills in once paper/live setups close with a P(win) attached.
              </div>
            ) : (
              <table className="min-w-full text-xs">
                <thead className="text-[10px] uppercase tracking-wide text-zinc-500">
                  <tr>
                    <th className="px-2 py-1 text-left">Bin</th>
                    <th className="px-2 py-1 text-right">n</th>
                    <th className="px-2 py-1 text-right">Predicted</th>
                    <th className="px-2 py-1 text-right">Realized</th>
                    <th className="px-2 py-1 text-right">Gap</th>
                    <th className="px-2 py-1 text-left">Calibration</th>
                  </tr>
                </thead>
                <tbody>
                  {calibration.data.bins.map((b) => {
                    // Signed gap: realized − predicted. |gap| > 0.10 → concerning.
                    const gapColor =
                      Math.abs(b.gap) >= 0.1
                        ? "text-red-300"
                        : Math.abs(b.gap) >= 0.05
                          ? "text-amber-300"
                          : "text-emerald-300";
                    return (
                      <tr
                        key={b.lo}
                        className="border-b border-zinc-800/60"
                      >
                        <td className="px-2 py-1 font-mono text-zinc-400">
                          {b.lo.toFixed(1)}–{b.hi.toFixed(1)}
                        </td>
                        <td className="px-2 py-1 text-right font-mono text-zinc-400">
                          {b.n}
                        </td>
                        <td className="px-2 py-1 text-right font-mono text-zinc-300">
                          {b.n > 0 ? b.mean_predicted.toFixed(3) : "—"}
                        </td>
                        <td className="px-2 py-1 text-right font-mono text-zinc-300">
                          {b.n > 0 ? b.realized_win_rate.toFixed(3) : "—"}
                        </td>
                        <td
                          className={`px-2 py-1 text-right font-mono ${gapColor}`}
                        >
                          {b.n > 0
                            ? (b.gap >= 0 ? "+" : "") + b.gap.toFixed(3)
                            : "—"}
                        </td>
                        <td className="px-2 py-1">
                          {b.n > 0 && (
                            <div className="relative h-2 w-32 rounded bg-zinc-800">
                              <div
                                className="absolute inset-y-0 left-0 rounded bg-blue-500/40"
                                style={{
                                  width: `${Math.min(100, b.mean_predicted * 100)}%`,
                                }}
                              />
                              <div
                                className="absolute inset-y-0 left-0 rounded bg-emerald-500/70"
                                style={{
                                  width: `${Math.min(100, b.realized_win_rate * 100)}%`,
                                  height: 3,
                                  top: 3,
                                }}
                              />
                            </div>
                          )}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            )}
          </div>
        </div>
      )}

      {/* Resolved breakers history */}
      {resolvedBreakers.length > 0 && (
        <div className="space-y-2">
          <div className="text-xs uppercase tracking-wide text-zinc-500">
            Resolved breaker history ({resolvedBreakers.length})
          </div>
          <div className="overflow-x-auto rounded-lg border border-zinc-800 bg-zinc-900/40">
            <table className="min-w-full">
              <thead className="bg-zinc-900/80 text-[10px] uppercase tracking-wide text-zinc-500">
                <tr>
                  <th className="px-2 py-2 text-left">Fired</th>
                  <th className="px-2 py-2 text-left">Resolved</th>
                  <th className="px-2 py-2 text-left">Action</th>
                  <th className="px-2 py-2 text-left">Model</th>
                  <th className="px-2 py-2 text-left">Reason</th>
                  <th className="px-2 py-2 text-left">By</th>
                  <th className="px-2 py-2 text-left">Note</th>
                </tr>
              </thead>
              <tbody>
                {resolvedBreakers.map((e) => (
                  <tr
                    key={e.id}
                    className="border-b border-zinc-800/60 text-xs"
                  >
                    <td className="px-2 py-1.5 font-mono text-zinc-500">
                      {fmtTs(e.fired_at)}
                    </td>
                    <td className="px-2 py-1.5 font-mono text-zinc-500">
                      {fmtTs(e.resolved_at)}
                    </td>
                    <td className="px-2 py-1.5 text-zinc-300">{e.action}</td>
                    <td className="px-2 py-1.5 font-mono text-zinc-500">
                      {e.model_version ?? "—"}
                    </td>
                    <td className="px-2 py-1.5 text-zinc-300">{e.reason}</td>
                    <td className="px-2 py-1.5 text-zinc-400">
                      {e.resolved_by ?? "—"}
                    </td>
                    <td className="px-2 py-1.5 text-zinc-500">
                      {e.resolution_note ?? ""}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}
