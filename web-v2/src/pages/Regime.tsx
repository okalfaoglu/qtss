import { FormEvent, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type { RegimeHud, RegimeKind } from "../lib/types";

const DEFAULTS = { venue: "binance", symbol: "BTCUSDT", timeframe: "1h" };

// Color registry for the kind pill
const KIND_BADGE: Record<RegimeKind, string> = {
  trending_up: "bg-emerald-500/20 text-emerald-300 border-emerald-500/40",
  trending_down: "bg-red-500/20 text-red-300 border-red-500/40",
  ranging: "bg-sky-500/15 text-sky-300 border-sky-500/30",
  squeeze: "bg-amber-500/15 text-amber-300 border-amber-500/40",
  volatile: "bg-fuchsia-500/15 text-fuchsia-300 border-fuchsia-500/40",
  uncertain: "bg-zinc-700/40 text-zinc-300 border-zinc-700",
};

const KIND_DOT: Record<string, string> = {
  trending_up: "bg-emerald-400",
  trending_down: "bg-red-400",
  ranging: "bg-sky-400",
  squeeze: "bg-amber-400",
  volatile: "bg-fuchsia-400",
  uncertain: "bg-zinc-500",
};

// =========================================================================
// Faz 11 types
// =========================================================================

interface DashboardEntry {
  symbol: string;
  intervals: { interval: string; regime: RegimeKind; confidence: number }[];
  dominant_regime: RegimeKind;
  confluence_score: number;
  is_transitioning: boolean;
}
interface Dashboard { generated_at: string; entries: DashboardEntry[] }

interface HeatmapData {
  generated_at: string;
  symbols: string[];
  intervals: string[];
  cells: { symbol: string; interval: string; regime: RegimeKind; confidence: number }[];
}

interface TransitionView {
  id: string; symbol: string; interval: string;
  from_regime: string; to_regime: string;
  transition_speed: number | null; confidence: number;
  confirming_indicators: string[];
  detected_at: string; resolved_at: string | null; was_correct: boolean | null;
}

interface PerformanceRow {
  regime: string; total: number; wins: number; win_rate: number; avg_pnl_pct: number;
}

// =========================================================================
// Shared components
// =========================================================================

function Pill({ kind }: { kind: RegimeKind }) {
  return (
    <span className={`rounded border px-2 py-0.5 text-xs uppercase ${KIND_BADGE[kind] ?? KIND_BADGE.uncertain}`}>
      {kind.replace("_", " ")}
    </span>
  );
}

function Dot({ regime }: { regime: string }) {
  return <span className={`inline-block h-3 w-3 rounded-full ${KIND_DOT[regime] ?? "bg-zinc-500"}`} title={regime} />;
}

function Stat({ label, value }: { label: string; value: string | number }) {
  return (
    <div>
      <div className="text-xs uppercase text-zinc-500">{label}</div>
      <div className="mt-0.5 font-mono text-sm text-zinc-100">{value}</div>
    </div>
  );
}

function Tabs({ tabs, active, onChange }: { tabs: string[]; active: string; onChange: (t: string) => void }) {
  return (
    <div className="flex gap-1 rounded-lg bg-zinc-900 p-1">
      {tabs.map((t) => (
        <button
          key={t}
          onClick={() => onChange(t)}
          className={`rounded px-3 py-1 text-xs font-medium transition ${
            active === t ? "bg-zinc-700 text-zinc-100" : "text-zinc-500 hover:text-zinc-300"
          }`}
        >
          {t}
        </button>
      ))}
    </div>
  );
}

// =========================================================================
// Dashboard tab
// =========================================================================

function RegimeDashboard() {
  const { data, isLoading, isError } = useQuery({
    queryKey: ["v2", "regime", "dashboard"],
    queryFn: () => apiFetch<Dashboard>("/v2/regime/dashboard"),
    refetchInterval: 30_000,
  });

  if (isLoading) return <div className="text-sm text-zinc-400">Loading dashboard…</div>;
  if (isError || !data) return <div className="text-sm text-red-400">Failed to load dashboard</div>;
  if (data.entries.length === 0) return <div className="text-sm text-zinc-500">No regime data yet. Waiting for snapshots…</div>;

  return (
    <div className="space-y-3">
      {data.entries.map((e) => (
        <div key={e.symbol} className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <span className="font-mono text-sm font-semibold text-zinc-100">{e.symbol}</span>
              <Pill kind={e.dominant_regime} />
              {e.is_transitioning && (
                <span className="rounded border border-amber-600/40 bg-amber-600/10 px-2 py-0.5 text-[10px] uppercase text-amber-300">
                  transitioning
                </span>
              )}
            </div>
            <div className="text-xs text-zinc-500">
              Confluence: <span className="font-mono text-zinc-300">{e.confluence_score.toFixed(2)}</span>
            </div>
          </div>
          <div className="mt-3 flex flex-wrap gap-3">
            {e.intervals.map((iv) => (
              <div key={iv.interval} className="flex items-center gap-1.5 rounded border border-zinc-800 px-2 py-1">
                <span className="text-xs text-zinc-500">{iv.interval}</span>
                <Dot regime={iv.regime} />
                <span className="font-mono text-[10px] text-zinc-400">{iv.confidence.toFixed(2)}</span>
              </div>
            ))}
          </div>
        </div>
      ))}
      <div className="text-[10px] text-zinc-600">Generated at {data.generated_at}</div>
    </div>
  );
}

// =========================================================================
// Heatmap tab
// =========================================================================

function RegimeHeatmap() {
  const { data, isLoading } = useQuery({
    queryKey: ["v2", "regime", "heatmap"],
    queryFn: () => apiFetch<HeatmapData>("/v2/regime/heatmap"),
    refetchInterval: 30_000,
  });

  if (isLoading || !data) return <div className="text-sm text-zinc-400">Loading heatmap…</div>;
  if (data.cells.length === 0) return <div className="text-sm text-zinc-500">No heatmap data yet.</div>;

  const cellMap = new Map<string, { regime: RegimeKind; confidence: number }>();
  for (const c of data.cells) cellMap.set(`${c.symbol}|${c.interval}`, c);

  return (
    <div className="overflow-x-auto">
      <table className="w-full text-xs">
        <thead>
          <tr className="border-b border-zinc-800 text-zinc-500">
            <th className="px-2 py-1 text-left font-medium">Symbol</th>
            {data.intervals.map((iv) => (
              <th key={iv} className="px-2 py-1 text-center font-medium">{iv}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {data.symbols.map((sym) => (
            <tr key={sym} className="border-b border-zinc-800/50 hover:bg-zinc-800/30">
              <td className="px-2 py-1.5 font-mono font-medium text-zinc-200">{sym}</td>
              {data.intervals.map((iv) => {
                const cell = cellMap.get(`${sym}|${iv}`);
                return (
                  <td key={iv} className="px-2 py-1.5 text-center">
                    {cell ? (
                      <div className="flex items-center justify-center gap-1">
                        <Dot regime={cell.regime} />
                        <span className="font-mono text-zinc-500">{cell.confidence.toFixed(2)}</span>
                      </div>
                    ) : (
                      <span className="text-zinc-700">—</span>
                    )}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      <div className="mt-3 flex flex-wrap gap-3 text-[10px] text-zinc-500">
        {Object.entries(KIND_DOT).map(([k, cls]) => (
          <div key={k} className="flex items-center gap-1">
            <span className={`inline-block h-2 w-2 rounded-full ${cls}`} />
            {k.replace("_", " ")}
          </div>
        ))}
      </div>
    </div>
  );
}

// =========================================================================
// Transitions tab
// =========================================================================

function RegimeTransitions() {
  const { data, isLoading } = useQuery({
    queryKey: ["v2", "regime", "transitions"],
    queryFn: () => apiFetch<TransitionView[]>("/v2/regime/transitions?limit=50"),
    refetchInterval: 15_000,
  });

  if (isLoading || !data) return <div className="text-sm text-zinc-400">Loading transitions…</div>;
  if (data.length === 0) return <div className="text-sm text-zinc-500">No transitions detected yet.</div>;

  return (
    <div className="space-y-2">
      {data.map((t) => (
        <div key={t.id} className="flex items-center gap-3 rounded-lg border border-zinc-800 bg-zinc-900/60 p-3 text-xs">
          <span className="font-mono font-semibold text-zinc-200">{t.symbol}</span>
          <span className="text-zinc-500">{t.interval}</span>
          <span className="flex items-center gap-1">
            <Pill kind={t.from_regime as RegimeKind} />
            <span className="text-zinc-600">→</span>
            <Pill kind={t.to_regime as RegimeKind} />
          </span>
          <span className="font-mono text-zinc-400">conf {t.confidence.toFixed(2)}</span>
          {t.confirming_indicators.length > 0 && (
            <span className="text-zinc-600">
              [{t.confirming_indicators.join(", ")}]
            </span>
          )}
          <span className="ml-auto text-zinc-600">{new Date(t.detected_at).toLocaleString()}</span>
          {t.resolved_at && (
            <span className={`rounded px-1.5 py-0.5 text-[10px] ${t.was_correct ? "bg-emerald-900/40 text-emerald-300" : "bg-red-900/40 text-red-300"}`}>
              {t.was_correct ? "correct" : "incorrect"}
            </span>
          )}
        </div>
      ))}
    </div>
  );
}

// =========================================================================
// Classic single-symbol tab (preserved from Faz 5)
// =========================================================================

function RegimeClassic() {
  const [form, setForm] = useState(DEFAULTS);
  const [submitted, setSubmitted] = useState(DEFAULTS);

  const query = useQuery({
    queryKey: ["v2", "regime", submitted],
    queryFn: () =>
      apiFetch<RegimeHud>(
        `/v2/regime/${submitted.venue}/${submitted.symbol}/${submitted.timeframe}`,
      ),
    refetchInterval: 10_000,
  });

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(form);
  };

  return (
    <div className="space-y-4">
      <form
        onSubmit={handleSubmit}
        className="flex flex-wrap items-end gap-3 rounded-lg border border-zinc-800 bg-zinc-900/60 p-4 text-sm"
      >
        <label className="flex flex-col gap-1">
          <span className="text-xs uppercase text-zinc-500">Venue</span>
          <input
            value={form.venue}
            onChange={(e) => setForm({ ...form, venue: e.target.value })}
            className="w-32 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 text-zinc-100"
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-xs uppercase text-zinc-500">Symbol</span>
          <input
            value={form.symbol}
            onChange={(e) => setForm({ ...form, symbol: e.target.value })}
            className="w-32 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 font-mono text-zinc-100"
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-xs uppercase text-zinc-500">Timeframe</span>
          <input
            value={form.timeframe}
            onChange={(e) => setForm({ ...form, timeframe: e.target.value })}
            className="w-20 rounded border border-zinc-700 bg-zinc-950 px-2 py-1 font-mono text-zinc-100"
          />
        </label>
        <button
          type="submit"
          className="rounded bg-emerald-500 px-3 py-1.5 text-sm font-medium text-zinc-950 hover:bg-emerald-400"
        >
          Load
        </button>
      </form>

      {query.isLoading && <div className="text-sm text-zinc-400">Loading regime…</div>}
      {query.isError && (
        <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
          Failed: {(query.error as Error).message}
        </div>
      )}
      {query.data && (
        <>
          <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-5">
            {query.data.current ? (
              <>
                <div className="flex items-center justify-between">
                  <Pill kind={query.data.current.kind} />
                  <div className="text-xs text-zinc-500">{query.data.current.at}</div>
                </div>
                <div className="mt-4 grid grid-cols-2 gap-4 md:grid-cols-3 lg:grid-cols-6">
                  <Stat label="Trend" value={query.data.current.trend_strength} />
                  <Stat label="Confidence" value={query.data.current.confidence.toFixed(2)} />
                  <Stat label="ADX" value={query.data.current.adx} />
                  <Stat label="BB width" value={query.data.current.bb_width} />
                  <Stat label="ATR %" value={query.data.current.atr_pct} />
                  <Stat label="Choppiness" value={query.data.current.choppiness} />
                </div>
              </>
            ) : (
              <div className="text-sm text-zinc-400">Engine warming up…</div>
            )}
          </div>

          <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-4">
            <div className="mb-3 text-xs uppercase tracking-wide text-zinc-500">
              History (newest last)
            </div>
            {query.data.history.length === 0 ? (
              <div className="text-sm text-zinc-500">No history yet.</div>
            ) : (
              <div className="flex flex-wrap gap-2">
                {query.data.history.map((p) => (
                  <div
                    key={p.at}
                    className="flex flex-col items-center gap-1 rounded border border-zinc-800 px-2 py-1"
                    title={`${p.at} · conf ${p.confidence.toFixed(2)}`}
                  >
                    <Pill kind={p.kind} />
                    <span className="font-mono text-[10px] text-zinc-500">
                      {p.confidence.toFixed(2)}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="text-xs text-zinc-500">Generated at {query.data.generated_at}</div>
        </>
      )}
    </div>
  );
}

// =========================================================================
// Performance tab
// =========================================================================

function RegimePerformance() {
  const [days, setDays] = useState(30);
  const { data, isLoading } = useQuery({
    queryKey: ["v2", "regime", "performance", days],
    queryFn: () => apiFetch<PerformanceRow[]>(`/v2/regime/performance?days=${days}`),
    refetchInterval: 60_000,
  });

  if (isLoading || !data) return <div className="text-sm text-zinc-400">Loading performance…</div>;
  if (data.length === 0) return <div className="text-sm text-zinc-500">No performance data yet.</div>;

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <span className="text-xs text-zinc-500">Period:</span>
        {[7, 30, 90].map((d) => (
          <button
            key={d}
            onClick={() => setDays(d)}
            className={`rounded px-2 py-0.5 text-xs font-medium ${days === d ? "bg-zinc-700 text-zinc-100" : "text-zinc-500 hover:text-zinc-300"}`}
          >
            {d}d
          </button>
        ))}
      </div>
      <table className="w-full text-xs">
        <thead>
          <tr className="border-b border-zinc-800 text-zinc-500">
            <th className="px-3 py-2 text-left font-medium">Regime</th>
            <th className="px-3 py-2 text-right font-medium">Total</th>
            <th className="px-3 py-2 text-right font-medium">Wins</th>
            <th className="px-3 py-2 text-right font-medium">Win Rate</th>
            <th className="px-3 py-2 text-right font-medium">Avg P&L %</th>
          </tr>
        </thead>
        <tbody>
          {data.map((r) => (
            <tr key={r.regime} className="border-b border-zinc-800/50 hover:bg-zinc-800/30">
              <td className="px-3 py-2">
                <div className="flex items-center gap-2">
                  <Dot regime={r.regime} />
                  <span className="text-zinc-200">{r.regime.replace("_", " ")}</span>
                </div>
              </td>
              <td className="px-3 py-2 text-right font-mono text-zinc-300">{r.total}</td>
              <td className="px-3 py-2 text-right font-mono text-zinc-300">{r.wins}</td>
              <td className={`px-3 py-2 text-right font-mono ${r.win_rate >= 50 ? "text-emerald-400" : "text-red-400"}`}>
                {r.win_rate.toFixed(1)}%
              </td>
              <td className={`px-3 py-2 text-right font-mono ${r.avg_pnl_pct >= 0 ? "text-emerald-400" : "text-red-400"}`}>
                {r.avg_pnl_pct >= 0 ? "+" : ""}{r.avg_pnl_pct.toFixed(2)}%
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// =========================================================================
// Main page with tabs
// =========================================================================

const TAB_LIST = ["Dashboard", "Heatmap", "Transitions", "Performance", "Single Symbol"];

export function Regime() {
  const [tab, setTab] = useState("Dashboard");

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-zinc-100">Regime Deep</h2>
        <Tabs tabs={TAB_LIST} active={tab} onChange={setTab} />
      </div>

      {tab === "Dashboard" && <RegimeDashboard />}
      {tab === "Heatmap" && <RegimeHeatmap />}
      {tab === "Transitions" && <RegimeTransitions />}
      {tab === "Performance" && <RegimePerformance />}
      {tab === "Single Symbol" && <RegimeClassic />}
    </div>
  );
}
