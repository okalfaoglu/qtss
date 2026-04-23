import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";

interface DetectionEntry {
  id: string;
  detected_at: string;
  exchange: string;
  symbol: string;
  timeframe: string;
  family: string;
  subkind: string;
  state: string;
  structural_score: number;
  confidence: number | null;
  invalidation_price: string;
  validated_at: string | null;
  mode: string;
  channel_scores: unknown | null;
}

interface DetectionsFeed {
  generated_at: string;
  entries: DetectionEntry[];
}

// Static option lists. Keeping these as data tables instead of inline
// JSX so adding a new family/state is a one-line change.
const FAMILIES = ["", "elliott", "harmonic", "classical", "wyckoff", "range", "custom"];
const STATES = ["", "forming", "confirmed", "invalidated", "completed"];
const MODES = ["", "live", "dry", "backtest"];

const STATE_BADGE: Record<string, string> = {
  forming: "bg-amber-500/15 text-amber-300 border-amber-500/30",
  confirmed: "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  invalidated: "bg-zinc-700/40 text-zinc-400 border-zinc-600/40",
  completed: "bg-sky-500/15 text-sky-300 border-sky-500/30",
};

function fmtTime(iso: string): string {
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

function fmtScore(v: number | null): string {
  if (v === null || v === undefined) return "—";
  return v.toFixed(3);
}

export function Detections() {
  const [exchange, setExchange] = useState("");
  const [symbol, setSymbol] = useState("");
  const [timeframe, setTimeframe] = useState("");
  const [family, setFamily] = useState("");
  const [state, setState] = useState("");
  const [mode, setMode] = useState("");

  const qs = useMemo(() => {
    const params = new URLSearchParams();
    if (exchange) params.set("exchange", exchange);
    if (symbol) params.set("symbol", symbol);
    if (timeframe) params.set("timeframe", timeframe);
    if (family) params.set("family", family);
    if (state) params.set("state", state);
    if (mode) params.set("mode", mode);
    params.set("limit", "200");
    return params.toString();
  }, [exchange, symbol, timeframe, family, state, mode]);

  const query = useQuery({
    queryKey: ["v2", "detections", qs],
    queryFn: () => apiFetch<DetectionsFeed>(`/v2/detections?${qs}`),
    refetchInterval: 5_000,
  });

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-end gap-3">
        <div>
          <div className="text-xs uppercase tracking-wide text-zinc-500">Exchange</div>
          <input
            value={exchange}
            onChange={(e) => setExchange(e.target.value.trim())}
            placeholder="binance"
            className="w-32 rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-sm text-zinc-100"
          />
        </div>
        <div>
          <div className="text-xs uppercase tracking-wide text-zinc-500">Symbol</div>
          <input
            value={symbol}
            onChange={(e) => setSymbol(e.target.value.trim().toUpperCase())}
            placeholder="BTCUSDT"
            className="w-32 rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-sm text-zinc-100"
          />
        </div>
        <div>
          <div className="text-xs uppercase tracking-wide text-zinc-500">Timeframe</div>
          <input
            value={timeframe}
            onChange={(e) => setTimeframe(e.target.value.trim())}
            placeholder="1h"
            className="w-24 rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-sm text-zinc-100"
          />
        </div>
        <div>
          <div className="text-xs uppercase tracking-wide text-zinc-500">Family</div>
          <select
            value={family}
            onChange={(e) => setFamily(e.target.value)}
            className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-sm text-zinc-100"
          >
            {FAMILIES.map((f) => (
              <option key={f} value={f}>
                {f || "any"}
              </option>
            ))}
          </select>
        </div>
        <div>
          <div className="text-xs uppercase tracking-wide text-zinc-500">State</div>
          <select
            value={state}
            onChange={(e) => setState(e.target.value)}
            className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-sm text-zinc-100"
          >
            {STATES.map((s) => (
              <option key={s} value={s}>
                {s || "any"}
              </option>
            ))}
          </select>
        </div>
        <div>
          <div className="text-xs uppercase tracking-wide text-zinc-500">Mode</div>
          <select
            value={mode}
            onChange={(e) => setMode(e.target.value)}
            className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-sm text-zinc-100"
          >
            {MODES.map((m) => (
              <option key={m} value={m}>
                {m || "any"}
              </option>
            ))}
          </select>
        </div>
      </div>

      {query.isLoading && <div className="text-sm text-zinc-400">Loading detections…</div>}
      {query.isError && (
        <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
          Failed to load detections: {(query.error as Error).message}
        </div>
      )}

      {query.data && (
        <div className="overflow-hidden rounded-lg border border-zinc-800 bg-zinc-900/60">
          <div className="flex items-baseline justify-between border-b border-zinc-800 px-3 py-2">
            <div className="text-xs uppercase tracking-wide text-zinc-500">
              {query.data.entries.length} detections
            </div>
            <div className="text-xs text-zinc-500">
              Generated at {query.data.generated_at}
            </div>
          </div>
          {query.data.entries.length === 0 ? (
            <div className="px-4 py-6 text-sm text-zinc-500">No detections match the filters.</div>
          ) : (
            <table className="w-full text-sm">
              <thead className="bg-zinc-900/80 text-xs uppercase text-zinc-500">
                <tr>
                  <th className="px-3 py-2 text-left">When</th>
                  <th className="px-3 py-2 text-left">Venue</th>
                  <th className="px-3 py-2 text-left">Symbol</th>
                  <th className="px-3 py-2 text-left">TF</th>
                  <th className="px-3 py-2 text-left">Family / Subkind</th>
                  <th className="px-3 py-2 text-left">State</th>
                  <th className="px-3 py-2 text-right">Structural</th>
                  <th className="px-3 py-2 text-right">Confidence</th>
                  <th className="px-3 py-2 text-right">Invalidation</th>
                  <th className="px-3 py-2 text-left">Mode</th>
                </tr>
              </thead>
              <tbody>
                {query.data.entries.map((d) => (
                  <tr key={d.id} className="border-t border-zinc-800/60">
                    <td className="px-3 py-2 font-mono text-xs text-zinc-400" title={d.detected_at}>
                      {fmtTime(d.detected_at)}
                    </td>
                    <td className="px-3 py-2 text-zinc-200">{d.exchange}</td>
                    <td className="px-3 py-2 font-mono text-zinc-100">{d.symbol}</td>
                    <td className="px-3 py-2 text-zinc-300">{d.timeframe}</td>
                    <td className="px-3 py-2 text-zinc-200">
                      {d.family}
                      <span className="text-zinc-500"> / {d.subkind}</span>
                    </td>
                    <td className="px-3 py-2">
                      <span
                        className={`inline-block rounded border px-2 py-0.5 text-xs uppercase ${
                          STATE_BADGE[d.state] ?? "border-zinc-700 text-zinc-300"
                        }`}
                      >
                        {d.state}
                      </span>
                    </td>
                    <td className="px-3 py-2 text-right font-mono text-zinc-200">
                      {fmtScore(d.structural_score)}
                    </td>
                    <td className="px-3 py-2 text-right font-mono text-zinc-100">
                      {fmtScore(d.confidence)}
                    </td>
                    <td className="px-3 py-2 text-right font-mono text-zinc-300">
                      {d.invalidation_price}
                    </td>
                    <td className="px-3 py-2 text-zinc-400">{d.mode}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}
    </div>
  );
}
