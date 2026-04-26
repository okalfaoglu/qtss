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

const TF_OPTIONS = ["", "15m", "1h", "4h", "1d", "1w"];
// Defaults match /v2/chart query semantics — operator can override
// via the inputs once we surface them, but the scoped path needs
// venue/segment to slot into the URL.
const DEFAULT_VENUE = "binance";
const DEFAULT_SEGMENT = "futures";

export default function BacktestStudio() {
  const [symbol, setSymbol] = useState("");
  const [tf, setTf] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);

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
                      }`}
                    >
                      <td className="px-2 py-1">
                        <div className="font-mono text-[11px]">{r.run_tag}</div>
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
              </div>
              <RunDetailView data={detail.data} />
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

function RunDetailView({ data }: { data: RunDetail }) {
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

      {/* Trade log path hint */}
      {report.trade_log_path !== undefined && report.trade_log_path !== null && (
        <div className="rounded border border-zinc-800 bg-zinc-900 p-3">
          <div className="text-xs font-semibold uppercase tracking-wider text-zinc-400">
            Per-trade JSONL
          </div>
          <div className="mt-1 font-mono text-[11px] text-zinc-200">
            {report.trade_log_path as string}
          </div>
          <div className="mt-2 text-[11px] text-zinc-400">
            Slice with DuckDB or pandas. See docs/BACKTEST.md for examples.
          </div>
        </div>
      )}
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
