import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";

// Faz 9C — Backtest results page.
// Consumes /v2/backtest/summary (aggregate + equity curve + alt_type breakdown)
// and /v2/backtest/setups (recent setups list, mode='backtest').
//
// The backtest dispatcher (`v2_backtest_setup_loop`) arms setups from
// historical detections and the same watcher machinery used by live/dry
// closes them. This page surfaces that closed-loop output so the operator
// can see whether historical replay produces profitable equity curves
// and which formation families (alt_type) dominate.

interface EquityPoint {
  ts: string;
  cum_pnl_pct: number;
  trade_count: number;
}

interface AltTypeStat {
  alt_type: string;
  count: number;
  wins: number;
  losses: number;
  hit_rate: number | null;
  avg_pnl_pct: number | null;
  total_pnl_pct: number | null;
}

interface BacktestSummary {
  generated_at: string;
  total_setups: number;
  armed: number;
  active: number;
  closed: number;
  wins: number;
  losses: number;
  hit_rate: number | null;
  avg_pnl_pct: number | null;
  total_pnl_pct: number | null;
  equity_curve: EquityPoint[];
  by_alt_type: AltTypeStat[];
}

interface BacktestSetupEntry {
  id: string;
  created_at: string;
  closed_at: string | null;
  exchange: string;
  symbol: string;
  timeframe: string;
  profile: string;
  alt_type: string | null;
  state: string;
  direction: string;
  entry_price: number | null;
  entry_sl: number | null;
  target_ref: number | null;
  close_price: number | null;
  close_reason: string | null;
  pnl_pct: number | null;
}

interface BacktestSetupsResponse {
  generated_at: string;
  entries: BacktestSetupEntry[];
}

function fmtPct(v: number | null | undefined, digits = 2): string {
  if (v === null || v === undefined || Number.isNaN(v)) return "—";
  return `${v.toFixed(digits)}%`;
}

function fmtRate(v: number | null | undefined): string {
  if (v === null || v === undefined) return "—";
  return `${(v * 100).toFixed(1)}%`;
}

function fmtNum(v: number | null | undefined, digits = 4): string {
  if (v === null || v === undefined) return "—";
  return v.toFixed(digits);
}

function fmtTs(iso: string | null): string {
  if (!iso) return "—";
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

const STATE_BADGE: Record<string, string> = {
  armed: "bg-sky-500/15 text-sky-300",
  active: "bg-emerald-500/15 text-emerald-300",
  closed: "bg-zinc-700/40 text-zinc-400",
};

const DIR_COLOR: Record<string, string> = {
  long: "text-emerald-300",
  short: "text-red-300",
};

function EquitySparkline({ points }: { points: EquityPoint[] }) {
  const width = 640;
  const height = 160;
  const padding = 8;
  const { path, zero, minY, maxY } = useMemo(() => {
    if (points.length < 2) {
      return { path: "", zero: height / 2, minY: 0, maxY: 0 };
    }
    const ys = points.map((p) => p.cum_pnl_pct);
    const minY = Math.min(0, ...ys);
    const maxY = Math.max(0, ...ys);
    const span = maxY - minY || 1;
    const n = points.length;
    const sx = (i: number) =>
      padding + (i / (n - 1)) * (width - 2 * padding);
    const sy = (y: number) =>
      height - padding - ((y - minY) / span) * (height - 2 * padding);
    const d = points
      .map((p, i) => `${i === 0 ? "M" : "L"}${sx(i).toFixed(1)},${sy(p.cum_pnl_pct).toFixed(1)}`)
      .join(" ");
    return { path: d, zero: sy(0), minY, maxY };
  }, [points]);

  if (points.length < 2) {
    return (
      <div className="flex h-40 items-center justify-center rounded border border-zinc-800 text-sm text-zinc-500">
        Equity curve için en az 2 kapanmış setup gerekli.
      </div>
    );
  }

  return (
    <div className="rounded border border-zinc-800 bg-zinc-950 p-2">
      <svg viewBox={`0 0 ${width} ${height}`} className="w-full">
        <line
          x1={0}
          x2={width}
          y1={zero}
          y2={zero}
          stroke="#3f3f46"
          strokeDasharray="4 4"
          strokeWidth={1}
        />
        <path d={path} stroke="#34d399" strokeWidth={1.5} fill="none" />
      </svg>
      <div className="flex justify-between px-1 text-xs text-zinc-500">
        <span>min {minY.toFixed(2)}%</span>
        <span>trade {points.length}</span>
        <span>max {maxY.toFixed(2)}%</span>
      </div>
    </div>
  );
}

export function Backtest() {
  const [altFilter, setAltFilter] = useState("");
  const [symbolFilter, setSymbolFilter] = useState("");
  const [tfFilter, setTfFilter] = useState("");

  const qs = useMemo(() => {
    const p = new URLSearchParams();
    if (altFilter.trim()) p.set("alt_type_like", altFilter.trim());
    if (symbolFilter.trim()) p.set("symbol", symbolFilter.trim().toUpperCase());
    if (tfFilter.trim()) p.set("timeframe", tfFilter.trim());
    const s = p.toString();
    return s ? `?${s}` : "";
  }, [altFilter, symbolFilter, tfFilter]);

  const summaryQuery = useQuery<BacktestSummary>({
    queryKey: ["backtest-summary", qs],
    queryFn: () => apiFetch<BacktestSummary>(`/v2/backtest/summary${qs}`),
    refetchInterval: 30_000,
  });

  const setupsQuery = useQuery<BacktestSetupsResponse>({
    queryKey: ["backtest-setups", qs],
    queryFn: () =>
      apiFetch<BacktestSetupsResponse>(`/v2/backtest/setups${qs}&limit=200`),
    refetchInterval: 30_000,
  });

  const summary = summaryQuery.data;

  return (
    <div className="flex flex-col gap-4 p-4 text-zinc-200">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold">Backtest Results</h1>
          <p className="text-xs text-zinc-500">
            mode=backtest setup'ları — dispatcher + watcher üzerinden akan
            historical replay sonuçları.
          </p>
        </div>
        <div className="flex gap-2 text-xs">
          <input
            className="rounded bg-zinc-900 px-2 py-1"
            placeholder="alt_type LIKE (wyckoff_%)"
            value={altFilter}
            onChange={(e) => setAltFilter(e.target.value)}
          />
          <input
            className="rounded bg-zinc-900 px-2 py-1"
            placeholder="symbol"
            value={symbolFilter}
            onChange={(e) => setSymbolFilter(e.target.value)}
          />
          <input
            className="rounded bg-zinc-900 px-2 py-1"
            placeholder="timeframe (1h…)"
            value={tfFilter}
            onChange={(e) => setTfFilter(e.target.value)}
          />
        </div>
      </header>

      {summaryQuery.isError && (
        <div className="rounded border border-red-500/30 bg-red-500/10 p-2 text-sm text-red-300">
          Summary yüklenemedi: {(summaryQuery.error as Error).message}
        </div>
      )}

      {summary && (
        <>
          <section className="grid grid-cols-2 gap-2 md:grid-cols-6">
            <StatCard label="Total" value={summary.total_setups.toString()} />
            <StatCard label="Armed" value={summary.armed.toString()} />
            <StatCard label="Active" value={summary.active.toString()} />
            <StatCard label="Closed" value={summary.closed.toString()} />
            <StatCard label="Hit rate" value={fmtRate(summary.hit_rate)} />
            <StatCard
              label="Total PnL%"
              value={fmtPct(summary.total_pnl_pct)}
              accent={
                summary.total_pnl_pct && summary.total_pnl_pct > 0
                  ? "text-emerald-300"
                  : summary.total_pnl_pct && summary.total_pnl_pct < 0
                  ? "text-red-300"
                  : undefined
              }
            />
          </section>

          <section>
            <h2 className="mb-1 text-sm font-semibold text-zinc-300">
              Equity curve (cumulative pnl_pct, additive)
            </h2>
            <EquitySparkline points={summary.equity_curve} />
          </section>

          <section>
            <h2 className="mb-1 text-sm font-semibold text-zinc-300">
              Per-formation (alt_type)
            </h2>
            <div className="overflow-x-auto rounded border border-zinc-800">
              <table className="w-full text-xs">
                <thead className="bg-zinc-900 text-zinc-400">
                  <tr>
                    <th className="px-2 py-1 text-left">alt_type</th>
                    <th className="px-2 py-1 text-right">count</th>
                    <th className="px-2 py-1 text-right">W</th>
                    <th className="px-2 py-1 text-right">L</th>
                    <th className="px-2 py-1 text-right">hit%</th>
                    <th className="px-2 py-1 text-right">avg pnl%</th>
                    <th className="px-2 py-1 text-right">total pnl%</th>
                  </tr>
                </thead>
                <tbody>
                  {summary.by_alt_type.length === 0 && (
                    <tr>
                      <td className="px-2 py-3 text-center text-zinc-500" colSpan={7}>
                        Kapanmış setup yok.
                      </td>
                    </tr>
                  )}
                  {summary.by_alt_type.map((row) => (
                    <tr key={row.alt_type} className="border-t border-zinc-800/60">
                      <td className="px-2 py-1 font-mono">{row.alt_type}</td>
                      <td className="px-2 py-1 text-right">{row.count}</td>
                      <td className="px-2 py-1 text-right text-emerald-300">{row.wins}</td>
                      <td className="px-2 py-1 text-right text-red-300">{row.losses}</td>
                      <td className="px-2 py-1 text-right">{fmtRate(row.hit_rate)}</td>
                      <td className="px-2 py-1 text-right">{fmtPct(row.avg_pnl_pct)}</td>
                      <td
                        className={`px-2 py-1 text-right ${
                          (row.total_pnl_pct ?? 0) > 0
                            ? "text-emerald-300"
                            : (row.total_pnl_pct ?? 0) < 0
                            ? "text-red-300"
                            : ""
                        }`}
                      >
                        {fmtPct(row.total_pnl_pct)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
        </>
      )}

      <section>
        <h2 className="mb-1 text-sm font-semibold text-zinc-300">
          Son 200 setup
        </h2>
        <div className="overflow-x-auto rounded border border-zinc-800">
          <table className="w-full text-xs">
            <thead className="bg-zinc-900 text-zinc-400">
              <tr>
                <th className="px-2 py-1 text-left">created</th>
                <th className="px-2 py-1 text-left">closed</th>
                <th className="px-2 py-1 text-left">sym</th>
                <th className="px-2 py-1 text-left">tf</th>
                <th className="px-2 py-1 text-left">prof</th>
                <th className="px-2 py-1 text-left">alt_type</th>
                <th className="px-2 py-1 text-left">state</th>
                <th className="px-2 py-1 text-left">dir</th>
                <th className="px-2 py-1 text-right">entry</th>
                <th className="px-2 py-1 text-right">SL</th>
                <th className="px-2 py-1 text-right">target</th>
                <th className="px-2 py-1 text-right">close</th>
                <th className="px-2 py-1 text-left">reason</th>
                <th className="px-2 py-1 text-right">pnl%</th>
              </tr>
            </thead>
            <tbody>
              {setupsQuery.data?.entries.length === 0 && (
                <tr>
                  <td className="px-2 py-3 text-center text-zinc-500" colSpan={14}>
                    Hiç setup yok. Dispatcher flag'ini açtın mı?
                    (<code>backtest.setup_loop.enabled</code>)
                  </td>
                </tr>
              )}
              {setupsQuery.data?.entries.map((s) => (
                <tr key={s.id} className="border-t border-zinc-800/60">
                  <td className="px-2 py-1 whitespace-nowrap text-zinc-400">
                    {fmtTs(s.created_at)}
                  </td>
                  <td className="px-2 py-1 whitespace-nowrap text-zinc-400">
                    {fmtTs(s.closed_at)}
                  </td>
                  <td className="px-2 py-1 font-mono">{s.symbol}</td>
                  <td className="px-2 py-1">{s.timeframe}</td>
                  <td className="px-2 py-1 uppercase">{s.profile}</td>
                  <td className="px-2 py-1 font-mono">{s.alt_type ?? "—"}</td>
                  <td className="px-2 py-1">
                    <span
                      className={`rounded px-1.5 py-0.5 ${
                        STATE_BADGE[s.state] ?? "bg-zinc-800/40 text-zinc-300"
                      }`}
                    >
                      {s.state}
                    </span>
                  </td>
                  <td className={`px-2 py-1 ${DIR_COLOR[s.direction] ?? ""}`}>
                    {s.direction}
                  </td>
                  <td className="px-2 py-1 text-right">{fmtNum(s.entry_price)}</td>
                  <td className="px-2 py-1 text-right">{fmtNum(s.entry_sl)}</td>
                  <td className="px-2 py-1 text-right">{fmtNum(s.target_ref)}</td>
                  <td className="px-2 py-1 text-right">{fmtNum(s.close_price)}</td>
                  <td className="px-2 py-1 text-zinc-400">{s.close_reason ?? "—"}</td>
                  <td
                    className={`px-2 py-1 text-right ${
                      (s.pnl_pct ?? 0) > 0
                        ? "text-emerald-300"
                        : (s.pnl_pct ?? 0) < 0
                        ? "text-red-300"
                        : ""
                    }`}
                  >
                    {fmtPct(s.pnl_pct)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}

function StatCard({
  label,
  value,
  accent,
}: {
  label: string;
  value: string;
  accent?: string;
}) {
  return (
    <div className="rounded border border-zinc-800 bg-zinc-950 p-2">
      <div className="text-[11px] uppercase tracking-wide text-zinc-500">
        {label}
      </div>
      <div className={`text-lg font-semibold ${accent ?? "text-zinc-100"}`}>
        {value}
      </div>
    </div>
  );
}
