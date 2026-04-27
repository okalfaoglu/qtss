// FAZ 26.6 — IQ Backtest Studio.
//
// Lists recent qtss-backtest::iq runs and shows a detail panel for
// the selected run. Read-only for now — dispatching new runs is
// queued for the next iteration (the backtest can take minutes;
// we'll need a background-task pattern for that).
//
// API contract (mirrors /v2/chart and /v2/elliott convention):
//   • Symbol+TF selected  → GET /v2/iq-backtest/{venue}/{sym}/{tf}/runs
//   • Cross-symbol view   → GET /v2/iq-backtest/runs?…filters…
//   • Single run detail   → GET /v2/iq-backtest/runs/{id}

import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { apiFetch } from "../lib/api";

interface RunRow {
  id: string;
  run_tag: string;
  polarity: "dip" | "top";
  exchange: string;
  segment: string;
  symbol: string;
  timeframe: string;
  start_time: string;
  end_time: string;
  bars_processed: number;
  total_trades: number;
  wins: number;
  losses: number;
  win_rate: number;
  profit_factor: number;
  net_pnl: string;
  max_drawdown_pct: number;
  created_at: string;
  trade_log_path: string | null;
}

interface RunDetail {
  config: Record<string, unknown>;
  report: Record<string, unknown>;
}

interface TradesResponse {
  run_id: string;
  run_tag: string;
  trade_log_path: string | null;
  returned: number;
  total_in_file: number;
  trades: Array<{
    trade?: Record<string, unknown>;
    attribution?: Record<string, unknown>;
  }>;
}

const TF_OPTIONS = ["", "15m", "1h", "4h", "1d", "1w"];
// Defaults match /v2/chart query semantics — operator can override
// via the inputs once we surface them, but the scoped path needs
// venue/segment to slot into the URL.
const DEFAULT_VENUE = "binance";
const DEFAULT_SEGMENT = "futures";

interface CompareResponse {
  left: { config: Record<string, unknown>; report: Record<string, unknown> };
  right: { config: Record<string, unknown>; report: Record<string, unknown> };
  delta: Record<string, { left: unknown; right: unknown }>;
}

export default function BacktestStudio() {
  const [symbol, setSymbol] = useState("");
  const [tf, setTf] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  // Run comparison — when set, the right pane swaps the single-run
  // detail for a side-by-side diff.
  const [compareWith, setCompareWith] = useState<string | null>(null);

  const runs = useQuery<RunRow[]>({
    queryKey: ["iq-backtest-runs", symbol, tf],
    queryFn: () => {
      // When the user has nailed down both symbol AND timeframe we
      // use the canonical {venue}/{symbol}/{tf} path — same shape
      // as /v2/chart and /v2/elliott. Anything looser falls back
      // to the global list with query filters so the operator can
      // sweep across symbols.
      if (symbol && tf) {
        const qs = new URLSearchParams();
        qs.set("segment", DEFAULT_SEGMENT);
        qs.set("limit", "100");
        return apiFetch(
          `/v2/iq-backtest/${DEFAULT_VENUE}/${symbol}/${tf}/runs?${qs.toString()}`,
        );
      }
      const qs = new URLSearchParams();
      if (symbol) qs.set("symbol", symbol);
      if (tf) qs.set("timeframe", tf);
      qs.set("limit", "100");
      return apiFetch(`/v2/iq-backtest/runs?${qs.toString()}`);
    },
    refetchInterval: 30_000,
  });

  const detail = useQuery<RunDetail>({
    queryKey: ["iq-backtest-run", selectedId],
    queryFn: () => apiFetch(`/v2/iq-backtest/runs/${selectedId}`),
    enabled: !!selectedId,
  });

  // POST /v2/iq-backtest/compare — server returns both run details
  // plus a sparse delta block keyed by headline fields.
  const compare = useQuery<CompareResponse>({
    queryKey: ["iq-backtest-compare", selectedId, compareWith],
    queryFn: () =>
      apiFetch("/v2/iq-backtest/compare", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ left: selectedId, right: compareWith }),
      }),
    enabled: !!selectedId && !!compareWith,
  });

  // 🔁 Re-run — clone the selected run's config and POST to dispatch.
  // Backend tokio task picks it up; we just need to refresh the list
  // afterwards. Status messaging is intentionally minimal — the new
  // row will simply appear in the list once the task persists.
  const queryClient = useQueryClient();
  const [dispatchStatus, setDispatchStatus] = useState<string | null>(null);
  async function rerunSelected() {
    if (!detail.data) return;
    const cfg = { ...(detail.data.config as Record<string, unknown>) };
    // Tag the new run so the user can find it in the list. Append
    // -rerun + timestamp; preserves the original config tag visually.
    const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
    cfg.run_tag = `${cfg.run_tag ?? "rerun"}-${stamp}`;
    setDispatchStatus("queueing…");
    try {
      const resp = await apiFetch<{ status: string; run_tag: string }>(
        "/v2/iq-backtest/dispatch",
        {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ config: cfg }),
        },
      );
      setDispatchStatus(`queued — ${resp.run_tag}`);
      // Soft-refresh after a few seconds; long-running backtests
      // surface in subsequent polls anyway thanks to refetchInterval.
      setTimeout(() => {
        queryClient.invalidateQueries({ queryKey: ["iq-backtest-runs"] });
      }, 3000);
    } catch (e) {
      setDispatchStatus(`failed: ${(e as Error).message}`);
    }
  }

  // Auto-select the first run on load.
  useEffect(() => {
    if (!selectedId && runs.data && runs.data.length > 0) {
      setSelectedId(runs.data[0].id);
    }
  }, [runs.data, selectedId]);

  const sortedRuns = useMemo(() => {
    return [...(runs.data ?? [])].sort(
      (a, b) =>
        new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
    );
  }, [runs.data]);

  return (
    <div className="flex h-screen flex-col bg-zinc-950 text-zinc-100">
      <header className="border-b border-zinc-800 px-4 py-2">
        <h1 className="text-lg font-semibold">IQ Backtest Studio</h1>
        <p className="text-xs text-zinc-400">
          FAZ 26 — replay + optimise IQ-D / IQ-T setups over historical bars.
        </p>
      </header>

      <div className="flex flex-1 overflow-hidden">
        {/* Left rail — filters + run list */}
        <aside className="flex w-[420px] flex-col border-r border-zinc-800">
          <div className="flex flex-col gap-2 border-b border-zinc-800 p-3">
            <label className="flex items-center gap-2 text-xs">
              <span className="w-16 text-zinc-400">Symbol</span>
              <input
                type="text"
                placeholder="BTCUSDT"
                value={symbol}
                onChange={(e) => setSymbol(e.target.value.toUpperCase())}
                className="flex-1 rounded border border-zinc-700 bg-zinc-900 px-2 py-1"
              />
            </label>
            <label className="flex items-center gap-2 text-xs">
              <span className="w-16 text-zinc-400">Timeframe</span>
              <select
                value={tf}
                onChange={(e) => setTf(e.target.value)}
                className="flex-1 rounded border border-zinc-700 bg-zinc-900 px-2 py-1"
              >
                {TF_OPTIONS.map((t) => (
                  <option key={t} value={t}>
                    {t || "(any)"}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <div className="flex-1 overflow-auto">
            {runs.isLoading ? (
              <div className="p-4 text-sm text-zinc-400">Loading…</div>
            ) : sortedRuns.length === 0 ? (
              <div className="p-4 text-sm text-zinc-500">
                No backtest runs yet. Run the CLI:
                <pre className="mt-2 whitespace-pre-wrap rounded bg-zinc-900 p-2 text-[11px]">
                  {`cargo run -p qtss-backtest --bin iq-backtest -- \\\n  --config crates/qtss-backtest/examples/btc_4h_dip.json`}
                </pre>
              </div>
            ) : (
              <table className="w-full text-xs">
                <thead className="sticky top-0 bg-zinc-900">
                  <tr className="text-left text-zinc-400">
                    <th className="px-2 py-1">Tag · Sym/TF</th>
                    <th className="px-2 py-1 text-right">Trades</th>
                    <th className="px-2 py-1 text-right">PnL</th>
                    <th className="px-2 py-1 text-right">DD%</th>
                  </tr>
                </thead>
                <tbody>
                  {sortedRuns.map((r) => (
                    <tr
                      key={r.id}
                      onClick={() => setSelectedId(r.id)}
                      className={`cursor-pointer border-t border-zinc-800 hover:bg-zinc-900 ${
                        r.id === selectedId ? "bg-zinc-800" : ""
                      } ${r.id === compareWith ? "ring-1 ring-amber-500/40" : ""}`}
                    >
                      <td className="px-2 py-1">
                        <div className="flex items-center justify-between gap-1">
                          <div className="font-mono text-[11px]">{r.run_tag}</div>
                          {selectedId && r.id !== selectedId && (
                            <button
                              type="button"
                              onClick={(e) => {
                                e.stopPropagation();
                                setCompareWith(
                                  compareWith === r.id ? null : r.id,
                                );
                              }}
                              className={`rounded border px-1 py-0 text-[9px] ${
                                compareWith === r.id
                                  ? "border-amber-500 text-amber-400"
                                  : "border-zinc-700 text-zinc-400 hover:border-amber-700"
                              }`}
                              title="Compare this run to the selected run"
                            >
                              {compareWith === r.id ? "✓ vs" : "vs"}
                            </button>
                          )}
                        </div>
                        <div className="text-[10px] text-zinc-400">
                          {r.symbol} {r.timeframe} ·{" "}
                          <span
                            className={
                              r.polarity === "dip"
                                ? "text-emerald-400"
                                : "text-rose-400"
                            }
                          >
                            {r.polarity}
                          </span>
                        </div>
                      </td>
                      <td className="px-2 py-1 text-right">
                        {r.total_trades}
                        <div className="text-[10px] text-zinc-500">
                          {r.wins}W/{r.losses}L
                        </div>
                      </td>
                      <td className="px-2 py-1 text-right font-mono">
                        <span
                          className={
                            Number(r.net_pnl) >= 0
                              ? "text-emerald-400"
                              : "text-rose-400"
                          }
                        >
                          {Number(r.net_pnl).toFixed(0)}
                        </span>
                      </td>
                      <td className="px-2 py-1 text-right font-mono text-zinc-400">
                        {r.max_drawdown_pct.toFixed(1)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </aside>

        {/* Detail panel */}
        <main className="flex flex-1 flex-col overflow-auto p-4">
          {detail.isLoading || !detail.data ? (
            <div className="text-sm text-zinc-500">
              {selectedId ? "Loading detail…" : "Select a run to view details."}
            </div>
          ) : compareWith ? (
            <>
              <div className="mb-3 flex items-center justify-between">
                <div className="text-sm font-semibold">
                  Run comparison
                </div>
                <button
                  type="button"
                  onClick={() => setCompareWith(null)}
                  className="rounded border border-zinc-700 bg-zinc-800 px-3 py-1 text-xs hover:bg-zinc-700"
                >
                  ✕ exit comparison
                </button>
              </div>
              {compare.isLoading || !compare.data ? (
                <div className="text-sm text-zinc-500">
                  Loading comparison…
                </div>
              ) : (
                <CompareView data={compare.data} />
              )}
            </>
          ) : (
            <>
              <div className="mb-3 flex items-center gap-3">
                <button
                  type="button"
                  onClick={rerunSelected}
                  className="rounded border border-zinc-700 bg-zinc-800 px-3 py-1 text-xs hover:bg-zinc-700"
                  title="Clone this run's config and queue a fresh backtest"
                >
                  🔁 Re-run with same config
                </button>
                {dispatchStatus && (
                  <span
                    className={`text-[11px] ${
                      dispatchStatus.startsWith("failed")
                        ? "text-rose-400"
                        : "text-emerald-400"
                    }`}
                  >
                    {dispatchStatus}
                  </span>
                )}
                <span className="text-[11px] text-zinc-500">
                  Pick another run's <span className="rounded border border-zinc-700 px-1">vs</span> button in the list to compare.
                </span>
              </div>
              <RunDetailView data={detail.data} runId={selectedId!} />
            </>
          )}
        </main>
      </div>
    </div>
  );
}

interface DataAvailabilityRow {
  channel: string;
  source: string;
  status: "full" | "partial" | "empty" | "missing" | string;
  rows_in_window: number;
  earliest: string | null;
  latest: string | null;
}

interface EquityPoint {
  trade_index: number;
  time: string;
  net_pnl_cum: string;
  equity: string;
  peak_equity: string;
  drawdown_pct: number;
}

/// Inline SVG line chart — equity curve + drawdown shading. Avoids
/// a chart-lib dependency so the bundle stays small. Rendering is
/// O(n) on points; for 10k+ trade runs we'd downsample but that's
/// not realistic here.
function EquityCurveChart({ points }: { points: EquityPoint[] }) {
  if (points.length === 0) return null;
  const W = 720;
  const H = 220;
  const PADX = 50;
  const PADY = 12;
  const equities = points.map((p) => Number(p.equity));
  const minEq = Math.min(...equities);
  const maxEq = Math.max(...equities);
  const range = Math.max(1, maxEq - minEq);
  const xStep =
    (W - PADX - 12) / Math.max(1, points.length - 1);
  const eqPath = points
    .map((p, i) => {
      const x = PADX + i * xStep;
      const y =
        PADY +
        ((maxEq - Number(p.equity)) / range) * (H - PADY * 2);
      return `${i === 0 ? "M" : "L"} ${x.toFixed(1)} ${y.toFixed(1)}`;
    })
    .join(" ");
  // Drawdown ribbon — area between peak and current equity, shaded
  // red to highlight underwater stretches.
  const ddPath =
    points
      .map((p, i) => {
        const x = PADX + i * xStep;
        const y =
          PADY +
          ((maxEq - Number(p.peak_equity)) / range) * (H - PADY * 2);
        return `${i === 0 ? "M" : "L"} ${x.toFixed(1)} ${y.toFixed(1)}`;
      })
      .join(" ") +
    " " +
    points
      .slice()
      .reverse()
      .map((p, i) => {
        const x =
          PADX + (points.length - 1 - i) * xStep;
        const y =
          PADY +
          ((maxEq - Number(p.equity)) / range) * (H - PADY * 2);
        return `L ${x.toFixed(1)} ${y.toFixed(1)}`;
      })
      .join(" ") +
    " Z";
  // Y-axis ticks (3 levels: max, mid, min).
  const ticks = [maxEq, (maxEq + minEq) / 2, minEq];
  const last = points[points.length - 1];
  const finalEq = Number(last.equity);
  const finalNet = Number(last.net_pnl_cum);
  const finalDd = last.drawdown_pct;
  return (
    <div className="rounded border border-zinc-800 bg-zinc-900 p-3">
      <div className="mb-2 flex items-center justify-between text-xs">
        <span className="font-semibold uppercase tracking-wider text-zinc-400">
          Equity curve
        </span>
        <span className="font-mono text-zinc-500">
          final: {finalEq.toFixed(0)} ·{" "}
          <span
            className={
              finalNet >= 0 ? "text-emerald-400" : "text-rose-400"
            }
          >
            {finalNet >= 0 ? "+" : ""}
            {finalNet.toFixed(0)}
          </span>{" "}
          · DD {finalDd.toFixed(1)}%
        </span>
      </div>
      <svg
        width={W}
        height={H}
        viewBox={`0 0 ${W} ${H}`}
        className="overflow-visible"
      >
        {/* Y-axis grid */}
        {ticks.map((t, i) => {
          const y =
            PADY + ((maxEq - t) / range) * (H - PADY * 2);
          return (
            <g key={i}>
              <line
                x1={PADX}
                y1={y}
                x2={W - 8}
                y2={y}
                stroke="#27272a"
                strokeDasharray="2 4"
              />
              <text
                x={PADX - 6}
                y={y + 3}
                fill="#71717a"
                fontSize="10"
                textAnchor="end"
                fontFamily="monospace"
              >
                {t.toFixed(0)}
              </text>
            </g>
          );
        })}
        {/* Drawdown ribbon */}
        <path d={ddPath} fill="rgba(239,68,68,0.10)" stroke="none" />
        {/* Equity line */}
        <path
          d={eqPath}
          fill="none"
          stroke="#10b981"
          strokeWidth={1.5}
        />
      </svg>
    </div>
  );
}

function RunDetailView({
  data,
  runId,
}: {
  data: RunDetail;
  runId: string;
}) {
  const cfg = data.config as Record<string, unknown>;
  const report = data.report as Record<string, unknown>;
  const universe = (cfg.universe ?? {}) as Record<string, string>;
  const lossReasons =
    (report.loss_reason_counts as Record<string, number>) ?? {};
  const avgLossComps =
    (report.avg_loss_components as Record<string, number>) ?? {};
  // BUG BACKTEST — pre-flight data availability matrix. Surfaces
  // immediately why a run produced 0 trades (e.g. missing
  // bar_indicator_snapshots → indicator_alignment scorer always 0).
  const availability = ((report.data_availability as { rows?: DataAvailabilityRow[] } | null)?.rows ?? null);
  const equityCurve = (report.equity_curve as EquityPoint[] | null) ?? [];

  const numField = (k: string, decimals = 2) => {
    const v = report[k];
    if (typeof v === "number") return v.toFixed(decimals);
    if (typeof v === "string") return Number(v).toFixed(decimals);
    return "—";
  };

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="rounded border border-zinc-800 bg-zinc-900 p-3">
        <div className="text-sm font-semibold">{cfg.run_tag as string}</div>
        <div className="text-xs text-zinc-400">
          {universe.exchange}/{universe.segment}/{universe.symbol}{" "}
          {universe.timeframe} · {(cfg.polarity as string) ?? "?"}
        </div>
        <div className="text-[11px] text-zinc-500">
          {universe.start_time} → {universe.end_time}
        </div>
      </div>

      {/* Headline numbers */}
      <div className="grid grid-cols-4 gap-3 text-sm">
        <Stat label="Trades" value={numField("total_trades", 0)} />
        <Stat
          label="Win Rate"
          value={`${(Number(report.win_rate ?? 0) * 100).toFixed(1)}%`}
        />
        <Stat label="Profit Factor" value={numField("profit_factor")} />
        <Stat label="Max DD" value={`${numField("max_drawdown_pct")}%`} />
        <Stat
          label="Net PnL"
          value={Number(report.net_pnl ?? 0).toFixed(2)}
          accent
        />
        <Stat
          label="Final Equity"
          value={Number(report.final_equity ?? 0).toFixed(2)}
        />
        <Stat
          label="Sharpe (per-trade)"
          value={
            report.sharpe_ratio !== null && report.sharpe_ratio !== undefined
              ? Number(report.sharpe_ratio).toFixed(3)
              : "—"
          }
        />
        <Stat label="Bars" value={numField("bars_processed", 0)} />
      </div>

      {/* Equity curve */}
      {equityCurve.length > 0 && <EquityCurveChart points={equityCurve} />}

      {/* Data availability matrix (BUG BACKTEST) */}
      {availability && availability.length > 0 && (
        <div className="rounded border border-zinc-800 bg-zinc-900 p-3">
          <div className="mb-2 flex items-center justify-between">
            <div className="text-xs font-semibold uppercase tracking-wider text-zinc-400">
              Data availability (pre-flight probe)
            </div>
            {availability.some(
              (r) => r.status === "missing" || r.status === "empty",
            ) && (
              <span className="text-[10px] text-amber-400">
                ⚠ critical gap detected — see scorer rows below
              </span>
            )}
          </div>
          <table className="w-full text-xs">
            <thead className="text-[10px] uppercase tracking-wider text-zinc-500">
              <tr>
                <th className="px-2 py-1 text-left">Channel</th>
                <th className="px-2 py-1 text-left">Source</th>
                <th className="px-2 py-1 text-left">Status</th>
                <th className="px-2 py-1 text-right">Rows</th>
              </tr>
            </thead>
            <tbody>
              {availability.map((r) => {
                const cls =
                  r.status === "full"
                    ? "text-emerald-400"
                    : r.status === "partial"
                      ? "text-amber-400"
                      : "text-rose-400";
                const glyph =
                  r.status === "full"
                    ? "✓"
                    : r.status === "partial"
                      ? "~"
                      : "✗";
                return (
                  <tr
                    key={r.channel}
                    className="border-t border-zinc-800"
                  >
                    <td className="px-2 py-1 font-mono">{r.channel}</td>
                    <td className="px-2 py-1 text-zinc-400">{r.source}</td>
                    <td className={`px-2 py-1 ${cls}`}>
                      {glyph} {r.status}
                    </td>
                    <td className="px-2 py-1 text-right font-mono">
                      {r.rows_in_window < 0
                        ? "—"
                        : r.rows_in_window.toLocaleString()}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {/* Loss reasons (with relative-share bars) */}
      {Object.keys(lossReasons).length > 0 && (
        <div className="rounded border border-zinc-800 bg-zinc-900 p-3">
          <div className="mb-2 text-xs font-semibold uppercase tracking-wider text-zinc-400">
            Loss reasons
          </div>
          {(() => {
            const total = Object.values(lossReasons).reduce(
              (s, v) => s + v,
              0,
            );
            return (
              <table className="w-full text-xs">
                <tbody>
                  {Object.entries(lossReasons)
                    .sort((a, b) => b[1] - a[1])
                    .map(([k, v]) => {
                      const pct = total > 0 ? (v / total) * 100 : 0;
                      return (
                        <tr
                          key={k}
                          className="border-t border-zinc-800"
                        >
                          <td className="px-2 py-1 font-mono">{k}</td>
                          <td className="px-2 py-1">
                            <div className="flex items-center gap-2">
                              <div className="h-2 flex-1 overflow-hidden rounded bg-zinc-800">
                                <div
                                  className="h-full bg-rose-500/70"
                                  style={{ width: `${pct.toFixed(1)}%` }}
                                />
                              </div>
                              <span className="w-12 text-right text-[10px] text-zinc-400">
                                {pct.toFixed(0)}%
                              </span>
                            </div>
                          </td>
                          <td className="px-2 py-1 text-right font-mono">
                            {v}
                          </td>
                        </tr>
                      );
                    })}
                </tbody>
              </table>
            );
          })()}
        </div>
      )}

      {/* Avg loser components (with magnitude bars) */}
      {Object.keys(avgLossComps).length > 0 && (
        <div className="rounded border border-zinc-800 bg-zinc-900 p-3">
          <div className="mb-2 text-xs font-semibold uppercase tracking-wider text-zinc-400">
            Avg score on losers (lowest = weakest channel)
          </div>
          {(() => {
            const max = Math.max(
              0.001,
              ...Object.values(avgLossComps).map((v) => Math.abs(v)),
            );
            return (
              <table className="w-full text-xs">
                <tbody>
                  {Object.entries(avgLossComps)
                    .sort((a, b) => a[1] - b[1])
                    .map(([k, v]) => {
                      const widthPct =
                        max > 0 ? (Math.abs(v) / max) * 100 : 0;
                      // Below 0.3 → red (weak), 0.3-0.6 → amber, ≥0.6 → green.
                      const tone =
                        v < 0.3
                          ? "bg-rose-500/70"
                          : v < 0.6
                            ? "bg-amber-500/70"
                            : "bg-emerald-500/70";
                      return (
                        <tr
                          key={k}
                          className="border-t border-zinc-800"
                        >
                          <td className="px-2 py-1 font-mono">{k}</td>
                          <td className="px-2 py-1">
                            <div className="flex items-center gap-2">
                              <div className="h-2 flex-1 overflow-hidden rounded bg-zinc-800">
                                <div
                                  className={`h-full ${tone}`}
                                  style={{ width: `${widthPct.toFixed(1)}%` }}
                                />
                              </div>
                            </div>
                          </td>
                          <td className="px-2 py-1 text-right font-mono">
                            {v.toFixed(3)}
                          </td>
                        </tr>
                      );
                    })}
                </tbody>
              </table>
            );
          })()}
        </div>
      )}

      {/* Trade timeline browser */}
      <TradeTimeline runId={runId} report={report} />
    </div>
  );
}

function CompareView({ data }: { data: CompareResponse }) {
  // Render the headline numbers that the server's delta map flagged
  // as different. Each row shows left | right with the side that
  // performed better tinted green for PnL-style fields.
  const fmt = (v: unknown) => {
    if (v === null || v === undefined) return "—";
    if (typeof v === "number") return v.toFixed(4);
    if (typeof v === "string") {
      const n = Number(v);
      return Number.isFinite(n) ? n.toFixed(4) : v;
    }
    return String(v);
  };
  const leftReport = data.left.report;
  const rightReport = data.right.report;
  const leftCfg = data.left.config as Record<string, unknown>;
  const rightCfg = data.right.config as Record<string, unknown>;
  const deltaKeys = Object.keys(data.delta);
  // For "higher is better" fields, tint the larger value green.
  // For "lower is better" (drawdown, losses), tint the smaller green.
  const lowerIsBetter = new Set([
    "max_drawdown_pct",
    "losses",
  ]);
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-3 text-xs">
        <div className="rounded border border-emerald-700/40 bg-emerald-900/10 p-3">
          <div className="text-[10px] uppercase tracking-wider text-emerald-400">
            Left (selected)
          </div>
          <div className="mt-1 font-mono text-sm">
            {leftCfg.run_tag as string}
          </div>
          <div className="text-[10px] text-zinc-400">
            {(leftCfg.universe as Record<string, string>)?.symbol}{" "}
            {(leftCfg.universe as Record<string, string>)?.timeframe} ·{" "}
            {leftCfg.polarity as string}
          </div>
        </div>
        <div className="rounded border border-amber-700/40 bg-amber-900/10 p-3">
          <div className="text-[10px] uppercase tracking-wider text-amber-400">
            Right (comparison)
          </div>
          <div className="mt-1 font-mono text-sm">
            {rightCfg.run_tag as string}
          </div>
          <div className="text-[10px] text-zinc-400">
            {(rightCfg.universe as Record<string, string>)?.symbol}{" "}
            {(rightCfg.universe as Record<string, string>)?.timeframe} ·{" "}
            {rightCfg.polarity as string}
          </div>
        </div>
      </div>

      <div className="rounded border border-zinc-800 bg-zinc-900 p-3">
        <div className="mb-2 text-xs font-semibold uppercase tracking-wider text-zinc-400">
          Headline diff ({deltaKeys.length} field
          {deltaKeys.length === 1 ? "" : "s"} differ)
        </div>
        {deltaKeys.length === 0 ? (
          <div className="text-[11px] text-zinc-500">
            All headline numbers match — this is unusual; check that you
            didn't pick the same run twice.
          </div>
        ) : (
          <table className="w-full text-xs">
            <thead className="text-[10px] uppercase tracking-wider text-zinc-500">
              <tr>
                <th className="px-2 py-1 text-left">Field</th>
                <th className="px-2 py-1 text-right">Left</th>
                <th className="px-2 py-1 text-right">Right</th>
                <th className="px-2 py-1 text-right">Δ</th>
              </tr>
            </thead>
            <tbody>
              {deltaKeys.map((k) => {
                const lv = Number(data.delta[k]?.left ?? 0);
                const rv = Number(data.delta[k]?.right ?? 0);
                const diff = rv - lv;
                const winner =
                  lowerIsBetter.has(k)
                    ? lv < rv
                      ? "left"
                      : "right"
                    : lv > rv
                      ? "left"
                      : "right";
                return (
                  <tr key={k} className="border-t border-zinc-800">
                    <td className="px-2 py-1 font-mono">{k}</td>
                    <td
                      className={`px-2 py-1 text-right font-mono ${
                        winner === "left"
                          ? "text-emerald-400"
                          : "text-zinc-300"
                      }`}
                    >
                      {fmt(data.delta[k]?.left)}
                    </td>
                    <td
                      className={`px-2 py-1 text-right font-mono ${
                        winner === "right"
                          ? "text-emerald-400"
                          : "text-zinc-300"
                      }`}
                    >
                      {fmt(data.delta[k]?.right)}
                    </td>
                    <td
                      className={`px-2 py-1 text-right font-mono ${
                        diff > 0
                          ? "text-emerald-400"
                          : diff < 0
                            ? "text-rose-400"
                            : "text-zinc-500"
                      }`}
                    >
                      {diff >= 0 ? "+" : ""}
                      {diff.toFixed(2)}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {/* Loss-reason side-by-side */}
      <div className="grid grid-cols-2 gap-3">
        {[
          { side: "left", report: leftReport, label: "Left losses" },
          { side: "right", report: rightReport, label: "Right losses" },
        ].map(({ side, report, label }) => {
          const reasons =
            (report.loss_reason_counts as Record<string, number>) ?? {};
          const total = Object.values(reasons).reduce(
            (s, v) => s + v,
            0,
          );
          return (
            <div
              key={side}
              className="rounded border border-zinc-800 bg-zinc-900 p-3"
            >
              <div className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-zinc-400">
                {label}
              </div>
              {Object.keys(reasons).length === 0 ? (
                <div className="text-[10px] text-zinc-500">
                  No losses recorded.
                </div>
              ) : (
                <table className="w-full text-[11px]">
                  <tbody>
                    {Object.entries(reasons)
                      .sort((a, b) => b[1] - a[1])
                      .map(([k, v]) => (
                        <tr
                          key={k}
                          className="border-t border-zinc-800"
                        >
                          <td className="px-2 py-1 font-mono">{k}</td>
                          <td className="px-2 py-1 text-right">{v}</td>
                          <td className="px-2 py-1 text-right text-zinc-500">
                            {total > 0
                              ? `${((v / total) * 100).toFixed(0)}%`
                              : "—"}
                          </td>
                        </tr>
                      ))}
                  </tbody>
                </table>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function TradeTimeline({
  runId,
  report,
}: {
  runId: string;
  report: Record<string, unknown>;
}) {
  const tradeLogPath = report.trade_log_path as string | null | undefined;
  const [outcomeFilter, setOutcomeFilter] = useState<string>("");
  const [reasonFilter, setReasonFilter] = useState<string>("");
  const trades = useQuery<TradesResponse>({
    queryKey: ["iq-backtest-trades", runId, outcomeFilter, reasonFilter],
    queryFn: () => {
      const qs = new URLSearchParams();
      qs.set("limit", "500");
      if (outcomeFilter) qs.set("outcome", outcomeFilter);
      if (reasonFilter) qs.set("loss_reason", reasonFilter);
      return apiFetch(
        `/v2/iq-backtest/runs/${runId}/trades?${qs.toString()}`,
      );
    },
    enabled: !!tradeLogPath,
  });

  if (!tradeLogPath) {
    return (
      <div className="rounded border border-zinc-800 bg-zinc-900 p-3">
        <div className="text-xs font-semibold uppercase tracking-wider text-zinc-400">
          Per-trade timeline
        </div>
        <div className="mt-2 text-[11px] text-zinc-500">
          This run was launched without --log; per-trade detail is not
          available. Re-run with --log to capture the JSONL stream.
        </div>
      </div>
    );
  }

  return (
    <div className="rounded border border-zinc-800 bg-zinc-900 p-3">
      <div className="mb-2 flex items-center justify-between">
        <div className="text-xs font-semibold uppercase tracking-wider text-zinc-400">
          Per-trade timeline
        </div>
        <div className="flex items-center gap-3">
          {trades.data && (
            <span className="text-[10px] text-zinc-500">
              {trades.data.returned}/{trades.data.total_in_file} trades
            </span>
          )}
          {/* CSV export — server streams the JSONL flattened to CSV. */}
          <a
            href={`/api/v2/iq-backtest/runs/${runId}/trades.csv`}
            className="rounded border border-zinc-700 bg-zinc-800 px-2 py-0.5 text-[10px] hover:bg-zinc-700"
            title="Download every trade as CSV (pandas / Excel)"
          >
            ⬇ CSV
          </a>
        </div>
      </div>
      <div className="mb-2 flex items-center gap-2 text-[11px]">
        <label className="flex items-center gap-1">
          <span className="text-zinc-500">Outcome</span>
          <select
            value={outcomeFilter}
            onChange={(e) => setOutcomeFilter(e.target.value)}
            className="rounded border border-zinc-700 bg-zinc-900 px-1 py-0.5"
          >
            <option value="">(any)</option>
            <option value="take_profit_full">take_profit_full</option>
            <option value="take_profit_partial">take_profit_partial</option>
            <option value="stop_loss">stop_loss</option>
            <option value="trailing_stop">trailing_stop</option>
            <option value="timeout">timeout</option>
          </select>
        </label>
        <label className="flex items-center gap-1">
          <span className="text-zinc-500">Loss reason</span>
          <select
            value={reasonFilter}
            onChange={(e) => setReasonFilter(e.target.value)}
            className="rounded border border-zinc-700 bg-zinc-900 px-1 py-0.5"
          >
            <option value="">(any)</option>
            <option value="StopLossNoTp">StopLossNoTp</option>
            <option value="StopLossAfterPartialTp">
              StopLossAfterPartialTp
            </option>
            <option value="TrailingStopAfterMfe">
              TrailingStopAfterMfe
            </option>
            <option value="TimeoutNoProgress">TimeoutNoProgress</option>
            <option value="TimeoutNegative">TimeoutNegative</option>
            <option value="MfeBeyondSlButNoTp">MfeBeyondSlButNoTp</option>
            <option value="CostsOnly">CostsOnly</option>
            <option value="InvalidationEvent">InvalidationEvent</option>
          </select>
        </label>
      </div>
      <div className="max-h-[400px] overflow-auto">
        <table className="w-full text-[11px]">
          <thead className="sticky top-0 bg-zinc-900 text-[10px] uppercase tracking-wider text-zinc-500">
            <tr>
              <th className="px-2 py-1 text-left">#</th>
              <th className="px-2 py-1 text-left">Entry</th>
              <th className="px-2 py-1 text-right">Entry $</th>
              <th className="px-2 py-1 text-left">Outcome</th>
              <th className="px-2 py-1 text-right">PnL</th>
              <th className="px-2 py-1 text-right">PnL %</th>
              <th className="px-2 py-1 text-right">Bars</th>
              <th className="px-2 py-1 text-right">MFE</th>
              <th className="px-2 py-1 text-right">MAE</th>
              <th className="px-2 py-1 text-left">Loss reason</th>
              <th className="px-2 py-1 text-right">Score</th>
            </tr>
          </thead>
          <tbody>
            {trades.data?.trades.map((row, i) => {
              const t =
                (row.trade as Record<string, unknown>) ?? {};
              const a =
                (row.attribution as Record<string, unknown>) ?? {};
              const pnl = Number(t.net_pnl ?? 0);
              const pnlPct = Number(t.net_pnl_pct ?? 0);
              const outcome = (t.outcome as string) ?? "—";
              const lossReason =
                (a.loss_reason as string | null) ?? "";
              const cls = pnl >= 0 ? "text-emerald-400" : "text-rose-400";
              return (
                <tr
                  key={(t.trade_id as string) ?? `row-${i}`}
                  className="border-t border-zinc-800 hover:bg-zinc-800/40"
                >
                  <td className="px-2 py-1 text-zinc-500">{i + 1}</td>
                  <td className="px-2 py-1 font-mono">
                    {(t.entry_time as string)?.slice(0, 16) ?? "—"}
                  </td>
                  <td className="px-2 py-1 text-right font-mono">
                    {Number(t.entry_price ?? 0).toFixed(2)}
                  </td>
                  <td className="px-2 py-1">{outcome}</td>
                  <td className={`px-2 py-1 text-right font-mono ${cls}`}>
                    {pnl >= 0 ? "+" : ""}
                    {pnl.toFixed(2)}
                  </td>
                  <td className={`px-2 py-1 text-right font-mono ${cls}`}>
                    {pnlPct >= 0 ? "+" : ""}
                    {pnlPct.toFixed(2)}%
                  </td>
                  <td className="px-2 py-1 text-right text-zinc-400">
                    {t.bars_held as number}
                  </td>
                  <td className="px-2 py-1 text-right font-mono text-emerald-400/80">
                    {Number(t.max_favorable_pct ?? 0).toFixed(2)}
                  </td>
                  <td className="px-2 py-1 text-right font-mono text-rose-400/80">
                    {Number(t.max_adverse_pct ?? 0).toFixed(2)}
                  </td>
                  <td className="px-2 py-1 text-zinc-300">{lossReason}</td>
                  <td className="px-2 py-1 text-right font-mono text-zinc-400">
                    {Number(t.entry_composite_score ?? 0).toFixed(2)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <div className="mt-2 font-mono text-[10px] text-zinc-500">
        {tradeLogPath}
      </div>
    </div>
  );
}

function Stat({
  label,
  value,
  accent,
}: {
  label: string;
  value: string;
  accent?: boolean;
}) {
  return (
    <div className="rounded border border-zinc-800 bg-zinc-900 p-3">
      <div className="text-[10px] uppercase tracking-wider text-zinc-400">
        {label}
      </div>
      <div
        className={`mt-1 font-mono text-base ${
          accent ? "text-emerald-300" : "text-zinc-100"
        }`}
      >
        {value}
      </div>
    </div>
  );
}
