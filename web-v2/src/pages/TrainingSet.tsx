import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";

// Faz 9.2.1/9.2.2 — Training Set Monitor.
// Tells the operator whether the Faz 9.3 LightGBM trainer has enough
// labeled data + feature coverage to kick off. Thresholds are resolved
// server-side from config_schema (setup.training_set.*).

type LabelBucket = { label: string; n: number };
type FeatureCoverage = { source: string; n: number };
type CloseReasonBucket = {
  reason: string;
  category: string | null;
  n: number;
};
type PnlSummary = {
  n_closed: number;
  n_win: number;
  n_loss: number;
  n_other: number;
  avg_rr: number | null;
  avg_pnl_pct: number | null;
  best_rr: number | null;
  worst_rr: number | null;
};

type SymbolBucket = {
  exchange: string;
  symbol: string;
  timeframe: string;
  n: number;
  n_win: number;
  n_loss: number;
  avg_pnl_pct: number | null;
};
type DirectionBucket = {
  direction: string;
  n: number;
  n_win: number;
  n_loss: number;
  hit_rate: number | null;
};

type TrainingSetStats = {
  total_setups: number;
  closed_setups: number;
  labeled_setups: number;
  setups_with_features: number;
  label_distribution: LabelBucket[];
  feature_coverage: FeatureCoverage[];
  close_reasons: CloseReasonBucket[];
  pnl: PnlSummary;
  per_symbol: SymbolBucket[];
  per_direction: DirectionBucket[];
};

// Category → color. `take_profit` green, `stop_loss` red, everything
// else neutral. Keeps the chip row scannable at a glance.
function categoryClass(cat: string | null): string {
  if (!cat) return "border-zinc-700 bg-zinc-800/60 text-zinc-300";
  const c = cat.toLowerCase();
  if (c.includes("take_profit") || c === "tp" || c === "win")
    return "border-emerald-500/40 bg-emerald-500/15 text-emerald-300";
  if (c.includes("stop") || c === "sl" || c === "loss")
    return "border-red-500/40 bg-red-500/15 text-red-300";
  if (c.includes("timeout") || c.includes("expire"))
    return "border-amber-500/40 bg-amber-500/15 text-amber-300";
  if (c.includes("invalid") || c.includes("manual"))
    return "border-sky-500/40 bg-sky-500/15 text-sky-300";
  return "border-zinc-700 bg-zinc-800/60 text-zinc-300";
}

function fmtNum(v: number | null | undefined, digits = 2): string {
  if (v == null || Number.isNaN(v)) return "—";
  return Number(v).toFixed(digits);
}

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

      {/* PnL & close-reason summary — kâr/zarar/stop breakdown */}
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-[1fr_2fr]">
        <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
          <div className="mb-2 text-[10px] uppercase tracking-wide text-zinc-500">
            PnL (closed slice, n={s.pnl.n_closed})
          </div>
          <div className="grid grid-cols-2 gap-y-1 text-xs">
            <div className="text-zinc-500">Win / Loss / Other</div>
            <div className="text-right font-mono">
              <span className="text-emerald-300">{s.pnl.n_win}</span>
              <span className="text-zinc-600"> / </span>
              <span className="text-red-300">{s.pnl.n_loss}</span>
              <span className="text-zinc-600"> / </span>
              <span className="text-zinc-400">{s.pnl.n_other}</span>
            </div>
            <div className="text-zinc-500">Hit rate</div>
            <div className="text-right font-mono text-zinc-100">
              {s.pnl.n_closed > 0
                ? `${((s.pnl.n_win / s.pnl.n_closed) * 100).toFixed(1)}%`
                : "—"}
            </div>
            <div className="text-zinc-500">Avg realized R</div>
            <div className="text-right font-mono text-zinc-100">
              {fmtNum(s.pnl.avg_rr)}
            </div>
            <div className="text-zinc-500">Avg pnl %</div>
            <div className="text-right font-mono text-zinc-100">
              {fmtNum(s.pnl.avg_pnl_pct)}%
            </div>
            <div className="text-zinc-500">Best / Worst R</div>
            <div className="text-right font-mono text-zinc-100">
              <span className="text-emerald-300">{fmtNum(s.pnl.best_rr)}</span>
              <span className="text-zinc-600"> / </span>
              <span className="text-red-300">{fmtNum(s.pnl.worst_rr)}</span>
            </div>
          </div>
        </div>

        <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
          <div className="mb-2 text-[10px] uppercase tracking-wide text-zinc-500">
            Close reasons (kâr / zarar / stop / timeout)
          </div>
          {s.close_reasons.length === 0 ? (
            <div className="text-xs text-zinc-500">
              No closed setups yet — outcome labeler hasn't written any rows.
            </div>
          ) : (
            <div className="flex flex-wrap gap-1.5">
              {s.close_reasons.map((b) => (
                <span
                  key={`${b.reason}:${b.category ?? ""}`}
                  className={`rounded border px-2 py-1 text-[11px] ${categoryClass(
                    b.category,
                  )}`}
                  title={
                    b.category
                      ? `category = ${b.category}`
                      : "no category — outcome labeler didn't tag"
                  }
                >
                  <span className="font-mono">{b.reason}</span>
                  <span className="ml-1.5 font-mono font-semibold">{b.n}</span>
                </span>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Long vs short balance — guards against directional bias in
          the training set (model learning a trend rather than confluence). */}
      <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
        <div className="mb-2 text-[10px] uppercase tracking-wide text-zinc-500">
          Direction balance (long / short)
        </div>
        {s.per_direction.length === 0 ? (
          <div className="text-xs text-zinc-500">
            No closed setups yet.
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
            {s.per_direction.map((d) => {
              const pct = s.closed_setups > 0 ? (d.n / s.closed_setups) * 100 : 0;
              const sideClass =
                d.direction === "long"
                  ? "text-emerald-300"
                  : d.direction === "short"
                    ? "text-red-300"
                    : "text-zinc-300";
              return (
                <div
                  key={d.direction}
                  className="rounded border border-zinc-800 bg-zinc-950/40 p-2"
                >
                  <div className="flex items-baseline justify-between">
                    <span className={`text-xs font-semibold uppercase ${sideClass}`}>
                      {d.direction}
                    </span>
                    <span className="font-mono text-[11px] text-zinc-500">
                      {pct.toFixed(1)}%
                    </span>
                  </div>
                  <div className="mt-1 flex items-baseline gap-3 text-xs">
                    <span className="font-mono text-zinc-100">{d.n}</span>
                    <span className="font-mono text-emerald-300">
                      {d.n_win}W
                    </span>
                    <span className="font-mono text-red-300">{d.n_loss}L</span>
                    <span className="ml-auto font-mono text-zinc-400">
                      hit {d.hit_rate == null ? "—" : `${(d.hit_rate * 100).toFixed(1)}%`}
                    </span>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Per-market breakdown — flags single-symbol overfit risk. */}
      <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
        <div className="mb-2 flex items-baseline justify-between">
          <div className="text-[10px] uppercase tracking-wide text-zinc-500">
            Per-market breakdown
          </div>
          <div className="text-[10px] text-zinc-500">
            {s.per_symbol.length} market{s.per_symbol.length === 1 ? "" : "s"}
          </div>
        </div>
        {s.per_symbol.length === 0 ? (
          <div className="text-xs text-zinc-500">No closed setups yet.</div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead>
                <tr className="text-left text-[10px] uppercase tracking-wide text-zinc-500">
                  <th className="py-1 pr-3">Market</th>
                  <th className="py-1 pr-3">TF</th>
                  <th className="py-1 pr-3 text-right">N</th>
                  <th className="py-1 pr-3 text-right">W</th>
                  <th className="py-1 pr-3 text-right">L</th>
                  <th className="py-1 pr-3 text-right">Hit %</th>
                  <th className="py-1 pr-3 text-right">Avg pnl %</th>
                  <th className="py-1 pr-3 text-right">Share</th>
                </tr>
              </thead>
              <tbody>
                {s.per_symbol.map((m) => {
                  const decisive = m.n_win + m.n_loss;
                  const hit = decisive > 0 ? (m.n_win / decisive) * 100 : null;
                  const share =
                    s.closed_setups > 0 ? (m.n / s.closed_setups) * 100 : 0;
                  // Highlight a single-symbol >50% concentration so the
                  // operator notices before the trainer overfits it.
                  const heavy = share > 50;
                  return (
                    <tr
                      key={`${m.exchange}:${m.symbol}:${m.timeframe}`}
                      className={`border-t border-zinc-800/60 ${
                        heavy ? "bg-amber-900/10" : ""
                      }`}
                    >
                      <td className="py-1 pr-3 font-mono text-zinc-100">
                        <span className="text-zinc-500">{m.exchange}:</span>
                        {m.symbol}
                      </td>
                      <td className="py-1 pr-3 font-mono text-zinc-400">
                        {m.timeframe}
                      </td>
                      <td className="py-1 pr-3 text-right font-mono text-zinc-100">
                        {m.n}
                      </td>
                      <td className="py-1 pr-3 text-right font-mono text-emerald-300">
                        {m.n_win}
                      </td>
                      <td className="py-1 pr-3 text-right font-mono text-red-300">
                        {m.n_loss}
                      </td>
                      <td className="py-1 pr-3 text-right font-mono text-zinc-100">
                        {hit == null ? "—" : hit.toFixed(1)}
                      </td>
                      <td className="py-1 pr-3 text-right font-mono text-zinc-100">
                        {fmtNum(m.avg_pnl_pct)}
                      </td>
                      <td className="py-1 pr-3 text-right font-mono text-zinc-400">
                        {share.toFixed(1)}%
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
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
