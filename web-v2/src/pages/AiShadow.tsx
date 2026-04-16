import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type {
  PredictionRow,
  PredictionSummary,
  ScoreBucket,
} from "../lib/types";

// ── Helpers ──────────────────────────────────────────────────────────

function fmtTs(iso: string | null): string {
  if (!iso) return "\u2014";
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

function fmtNum(v: number | null | undefined, digits = 2): string {
  if (v === null || v === undefined) return "\u2014";
  return v.toFixed(digits);
}

const DECISION_BADGE: Record<string, string> = {
  pass: "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  block: "bg-red-500/15 text-red-300 border-red-500/30",
  shadow: "bg-zinc-700/40 text-zinc-400 border-zinc-600/40",
};

function DecisionBadge({ decision }: { decision: string }) {
  const cls =
    DECISION_BADGE[decision] ??
    "bg-zinc-800/40 text-zinc-300 border-zinc-700";
  return (
    <span
      className={`inline-block rounded border px-1.5 py-0.5 text-[10px] font-semibold uppercase ${cls}`}
    >
      {decision}
    </span>
  );
}

function AiScoreChip({ score }: { score: number | null }) {
  if (score == null || Number.isNaN(score)) {
    return <span className="font-mono text-[11px] text-zinc-600">{"\u2014"}</span>;
  }
  const pct = score * 100;
  const cls =
    score >= 0.65
      ? "border-emerald-500/40 bg-emerald-500/15 text-emerald-300"
      : score >= 0.55
        ? "border-sky-500/40 bg-sky-500/15 text-sky-300"
        : score >= 0.45
          ? "border-amber-500/40 bg-amber-500/15 text-amber-300"
          : "border-red-500/40 bg-red-500/15 text-red-300";
  return (
    <span
      className={`inline-block rounded border px-1.5 py-0.5 font-mono text-[10px] ${cls}`}
      title={`P(win) = ${pct.toFixed(1)}%`}
    >
      {pct.toFixed(0)}%
    </span>
  );
}

// ── Stat chip for the header strip ───────────────────────────────────

function StatChip({
  label,
  value,
  color,
}: {
  label: string;
  value: string | number;
  color?: string;
}) {
  const border = color ?? "border-zinc-700";
  const text = color?.replace("border-", "text-") ?? "text-zinc-200";
  return (
    <div
      className={`rounded-lg border ${border} bg-zinc-900/60 px-4 py-2 text-center`}
    >
      <div className={`text-lg font-bold ${text}`}>{value}</div>
      <div className="text-[10px] uppercase tracking-wider text-zinc-500">
        {label}
      </div>
    </div>
  );
}

// ── SHAP bar chart ───────────────────────────────────────────────────

function ShapChart({
  items,
}: {
  items: { feature: string; value: number; contribution: number }[];
}) {
  const maxAbs = useMemo(
    () => Math.max(...items.map((i) => Math.abs(i.contribution)), 0.001),
    [items],
  );
  return (
    <div className="space-y-1">
      {items.map((item) => {
        const pct = (Math.abs(item.contribution) / maxAbs) * 100;
        const positive = item.contribution >= 0;
        return (
          <div key={item.feature} className="flex items-center gap-2 text-[11px]">
            <span className="w-28 truncate text-right text-zinc-400" title={item.feature}>
              {item.feature}
            </span>
            <div className="relative h-3 flex-1 rounded bg-zinc-800">
              <div
                className={`absolute inset-y-0 left-0 rounded ${
                  positive ? "bg-emerald-500/60" : "bg-red-500/60"
                }`}
                style={{ width: `${pct}%` }}
              />
            </div>
            <span className="w-14 text-right font-mono text-zinc-500">
              {item.contribution.toFixed(3)}
            </span>
          </div>
        );
      })}
    </div>
  );
}

// ── Score distribution histogram ─────────────────────────────────────

function ScoreHistogram({ buckets }: { buckets: ScoreBucket[] }) {
  const maxN = useMemo(
    () => Math.max(...buckets.map((b) => b.n), 1),
    [buckets],
  );
  return (
    <div className="flex items-end gap-px" style={{ height: 120 }}>
      {buckets.map((b) => {
        const h = (b.n / maxN) * 100;
        const passPct = b.n > 0 ? (b.n_pass / b.n) * 100 : 0;
        const blockPct = b.n > 0 ? (b.n_block / b.n) * 100 : 0;
        return (
          <div
            key={b.bucket}
            className="group relative flex-1"
            title={`${b.bucket.toFixed(2)}: ${b.n} (P:${b.n_pass} B:${b.n_block} S:${b.n_shadow})`}
          >
            <div
              className="relative w-full overflow-hidden rounded-t"
              style={{ height: `${h}%` }}
            >
              <div
                className="absolute bottom-0 left-0 right-0 bg-emerald-500/60"
                style={{ height: `${passPct}%` }}
              />
              <div
                className="absolute left-0 right-0 bg-red-500/60"
                style={{
                  bottom: `${passPct}%`,
                  height: `${blockPct}%`,
                }}
              />
              <div
                className="absolute left-0 right-0 bg-zinc-600/60"
                style={{
                  bottom: `${passPct + blockPct}%`,
                  height: `${100 - passPct - blockPct}%`,
                }}
              />
            </div>
          </div>
        );
      })}
    </div>
  );
}

// ── Main page ────────────────────────────────────────────────────────

// ── Circuit breaker status chip (Faz 9.4.3) ────────────────────────

const BREAKER_STYLE: Record<string, string> = {
  closed: "border-emerald-500/40 bg-emerald-500/15 text-emerald-300",
  open: "border-red-500/40 bg-red-500/15 text-red-300 animate-pulse",
  half_open: "border-amber-500/40 bg-amber-500/15 text-amber-300",
};

function BreakerChip({ state }: { state: string }) {
  const cls = BREAKER_STYLE[state] ?? BREAKER_STYLE.closed;
  return (
    <span
      className={`inline-block rounded border px-2 py-0.5 text-[10px] font-semibold uppercase ${cls}`}
      title={`Circuit breaker: ${state}`}
    >
      CB: {state.replace("_", " ")}
    </span>
  );
}

function GateRampBadge({ pct }: { pct: number }) {
  const display = `${(pct * 100).toFixed(0)}%`;
  const cls =
    pct >= 1.0
      ? "border-emerald-500/40 bg-emerald-500/15 text-emerald-300"
      : pct > 0
        ? "border-sky-500/40 bg-sky-500/15 text-sky-300"
        : "border-zinc-600 bg-zinc-800/40 text-zinc-400";
  return (
    <span
      className={`inline-block rounded border px-2 py-0.5 text-[10px] font-semibold ${cls}`}
      title={`Gate ramp: ${display} of setups go through AI gate`}
    >
      Gate: {display}
    </span>
  );
}

// ── Main page ────────────────────────────────────────────────────────

export function AiShadow() {
  const [decisionFilter, setDecisionFilter] = useState<string>("");
  const [symbolFilter, setSymbolFilter] = useState<string>("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [hours] = useState(24);

  // Breaker status (Faz 9.4.3 / 9.4.4)
  const { data: breakerData } = useQuery({
    queryKey: ["ml-predictions-breaker"],
    queryFn: () =>
      apiFetch<{ state: string; gate_pct: number }>("/v2/ml/predictions/breaker"),
    refetchInterval: 15_000,
  });

  // Feed
  const feedParams = new URLSearchParams();
  if (decisionFilter) feedParams.set("decision", decisionFilter);
  if (symbolFilter) feedParams.set("symbol", symbolFilter);
  feedParams.set("limit", "100");
  const feedQs = feedParams.toString();

  const { data: feedData } = useQuery({
    queryKey: ["ml-predictions-feed", feedQs],
    queryFn: () =>
      apiFetch<{ generated_at: string; entries: PredictionRow[] }>(
        `/v2/ml/predictions?${feedQs}`,
      ),
    refetchInterval: 10_000,
  });

  // Summary
  const { data: summaryData } = useQuery({
    queryKey: ["ml-predictions-summary", hours],
    queryFn: () =>
      apiFetch<{ generated_at: string; summary: PredictionSummary }>(
        `/v2/ml/predictions/summary?hours=${hours}`,
      ),
    refetchInterval: 30_000,
  });

  // Distribution
  const { data: distData } = useQuery({
    queryKey: ["ml-predictions-dist", hours],
    queryFn: () =>
      apiFetch<{ generated_at: string; buckets: ScoreBucket[] }>(
        `/v2/ml/predictions/distribution?hours=${hours}`,
      ),
    refetchInterval: 30_000,
  });

  const entries = feedData?.entries ?? [];
  const summary = summaryData?.summary;
  const selected = entries.find((e) => e.id === selectedId) ?? null;

  return (
    <div className="space-y-4 p-4">
      <div className="flex items-center gap-3">
        <h1 className="text-lg font-semibold text-zinc-100">AI Shadow</h1>
        {breakerData && <BreakerChip state={breakerData.state} />}
        {breakerData && <GateRampBadge pct={breakerData.gate_pct} />}
      </div>

      {/* ── Header stat chips ─────────────────────────────────── */}
      {summary && (
        <div className="grid grid-cols-3 gap-3 sm:grid-cols-6">
          <StatChip label={`Total (${hours}h)`} value={summary.total} />
          <StatChip label="Pass" value={summary.n_pass} color="border-emerald-500/40" />
          <StatChip label="Block" value={summary.n_block} color="border-red-500/40" />
          <StatChip label="Shadow" value={summary.n_shadow} color="border-zinc-600" />
          <StatChip label="Avg Score" value={fmtNum(summary.avg_score, 3)} />
          <StatChip
            label="Avg Latency"
            value={summary.avg_latency_ms != null ? `${fmtNum(summary.avg_latency_ms, 0)} ms` : "\u2014"}
          />
        </div>
      )}

      {/* ── Counterfactual banner ─────────────────────────────── */}
      {summary && summary.block_with_outcome > 0 && (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/5 px-4 py-2 text-sm text-amber-200">
          Counterfactual: AI blocked {summary.n_block} setups in last {hours}h.
          Of those with outcomes, {summary.block_wouldve_won}/{summary.block_with_outcome} would
          have been profitable.
          {summary.avg_pnl_pass != null && (
            <span className="ml-2 text-zinc-400">
              Avg PnL pass: {fmtNum(summary.avg_pnl_pass)}% | block: {fmtNum(summary.avg_pnl_block)}%
            </span>
          )}
        </div>
      )}

      {/* ── Filters ───────────────────────────────────────────── */}
      <div className="flex items-center gap-3">
        <select
          value={decisionFilter}
          onChange={(e) => setDecisionFilter(e.target.value)}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-xs text-zinc-300"
        >
          <option value="">All decisions</option>
          <option value="pass">Pass</option>
          <option value="block">Block</option>
          <option value="shadow">Shadow</option>
        </select>
        <input
          type="text"
          placeholder="Symbol filter..."
          value={symbolFilter}
          onChange={(e) => setSymbolFilter(e.target.value)}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-xs text-zinc-300 placeholder-zinc-600"
        />
      </div>

      {/* ── Two-column body ───────────────────────────────────── */}
      <div className="flex gap-4">
        {/* Left — table */}
        <div className="flex-[2] overflow-auto">
          <table className="w-full text-left text-xs text-zinc-300">
            <thead>
              <tr className="border-b border-zinc-800 text-[10px] uppercase tracking-wider text-zinc-500">
                <th className="px-2 py-1">Time</th>
                <th className="px-2 py-1">Symbol</th>
                <th className="px-2 py-1">TF</th>
                <th className="px-2 py-1">Score</th>
                <th className="px-2 py-1">Decision</th>
                <th className="px-2 py-1">Latency</th>
                <th className="px-2 py-1">Model</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((e) => (
                <tr
                  key={e.id}
                  onClick={() => setSelectedId(e.id)}
                  className={`cursor-pointer border-b border-zinc-800/60 hover:bg-zinc-800/30 ${
                    selectedId === e.id ? "bg-emerald-500/5" : ""
                  }`}
                >
                  <td className="whitespace-nowrap px-2 py-1 font-mono">
                    {fmtTs(e.inference_ts)}
                  </td>
                  <td className="px-2 py-1">{e.symbol}</td>
                  <td className="px-2 py-1">{e.timeframe}</td>
                  <td className="px-2 py-1">
                    <AiScoreChip score={e.score} />
                  </td>
                  <td className="px-2 py-1">
                    <DecisionBadge decision={e.decision} />
                  </td>
                  <td className="px-2 py-1 font-mono">{e.latency_ms}ms</td>
                  <td className="px-2 py-1 text-zinc-500">{e.model_version}</td>
                </tr>
              ))}
              {entries.length === 0 && (
                <tr>
                  <td colSpan={7} className="px-2 py-8 text-center text-zinc-600">
                    No predictions found.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        {/* Right — detail panel */}
        <div className="flex-1 rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
          {selected ? (
            <div className="space-y-4">
              <h3 className="text-sm font-semibold text-zinc-200">
                {selected.symbol} &middot; {selected.timeframe}
              </h3>

              {/* Score vs threshold bar */}
              <div>
                <div className="mb-1 text-[10px] uppercase tracking-wider text-zinc-500">
                  Score vs Threshold
                </div>
                <div className="relative h-4 rounded bg-zinc-800">
                  {/* threshold marker */}
                  <div
                    className="absolute top-0 bottom-0 w-px bg-amber-400"
                    style={{ left: `${selected.threshold * 100}%` }}
                    title={`Threshold: ${selected.threshold.toFixed(3)}`}
                  />
                  {/* score bar */}
                  <div
                    className={`absolute inset-y-0 left-0 rounded ${
                      selected.score >= selected.threshold
                        ? "bg-emerald-500/60"
                        : "bg-red-500/60"
                    }`}
                    style={{ width: `${selected.score * 100}%` }}
                  />
                </div>
                <div className="mt-0.5 flex justify-between text-[10px] text-zinc-500">
                  <span>Score: {selected.score.toFixed(3)}</span>
                  <span>Thr: {selected.threshold.toFixed(3)}</span>
                </div>
              </div>

              {/* SHAP top 10 */}
              {selected.shap_top10 && selected.shap_top10.length > 0 && (
                <div>
                  <div className="mb-1 text-[10px] uppercase tracking-wider text-zinc-500">
                    SHAP Top 10
                  </div>
                  <ShapChart items={selected.shap_top10} />
                </div>
              )}

              {/* Linked setup */}
              {selected.setup_id && (
                <div className="text-[11px] text-zinc-500">
                  Setup: <span className="font-mono text-zinc-400">{selected.setup_id}</span>
                </div>
              )}

              <div className="text-[10px] text-zinc-600">
                Gate: {selected.gate_enabled ? "enabled" : "disabled"} | ID: {selected.id.slice(0, 8)}
              </div>
            </div>
          ) : (
            <div className="flex h-32 items-center justify-center text-xs text-zinc-600">
              Click a prediction to view details
            </div>
          )}
        </div>
      </div>

      {/* ── Score distribution histogram ──────────────────────── */}
      {distData && distData.buckets.length > 0 && (
        <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
          <div className="mb-2 text-[10px] uppercase tracking-wider text-zinc-500">
            Score Distribution (last {hours}h)
          </div>
          <ScoreHistogram buckets={distData.buckets} />
          <div className="mt-1 flex justify-between text-[9px] text-zinc-600">
            <span>0.00</span>
            <span>0.50</span>
            <span>1.00</span>
          </div>
          <div className="mt-1 flex items-center gap-3 text-[10px] text-zinc-500">
            <span className="flex items-center gap-1">
              <span className="inline-block h-2 w-2 rounded bg-emerald-500/60" /> Pass
            </span>
            <span className="flex items-center gap-1">
              <span className="inline-block h-2 w-2 rounded bg-red-500/60" /> Block
            </span>
            <span className="flex items-center gap-1">
              <span className="inline-block h-2 w-2 rounded bg-zinc-600/60" /> Shadow
            </span>
          </div>
        </div>
      )}
    </div>
  );
}
