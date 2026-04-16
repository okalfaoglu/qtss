import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";

// Faz 9.1.3 — Confluence Inspector.
// Surfaces qtss_v2_setup_rejections so operators can answer
// "why didn't we trade X?" for every vetoed candidate.

type RejectionEntry = {
  id: string;
  created_at: string;
  venue_class: string;
  exchange: string;
  symbol: string;
  timeframe: string;
  profile: string;
  direction: string;
  reject_reason: string;
  confluence_id: string | null;
  raw_meta: unknown;
};

type RejectionFeed = {
  generated_at: string;
  entries: RejectionEntry[];
};

type ReasonBucket = { reason: string; n: number };

type RejectionSummary = {
  generated_at: string;
  since_hours: number;
  venue_class: string | null;
  total: number;
  by_reason: ReasonBucket[];
};

const WINDOWS: Array<{ hours: number; label: string }> = [
  { hours: 1, label: "Last 1h" },
  { hours: 6, label: "Last 6h" },
  { hours: 24, label: "Last 24h" },
  { hours: 24 * 7, label: "Last 7d" },
  { hours: 24 * 30, label: "Last 30d" },
];

const VENUES = ["", "crypto", "bist"];

const REASON_LABEL: Record<string, string> = {
  total_risk_cap: "Total risk cap",
  max_concurrent: "Max concurrent",
  correlation_cap: "Correlation cap",
  commission_gate: "Commission gate",
  gate_kill_switch: "Kill switch",
  gate_stale_data: "Stale data",
  gate_news_blackout: "News blackout",
  gate_regime_opposite: "Regime opposite",
  gate_direction_consensus: "Direction consensus",
  gate_below_min_score: "Below min score",
  gate_no_direction: "No direction",
};

function reasonBadge(reason: string): string {
  if (reason.startsWith("gate_")) {
    return "bg-amber-500/15 text-amber-300 border-amber-500/30";
  }
  if (reason === "commission_gate") {
    return "bg-fuchsia-500/15 text-fuchsia-300 border-fuchsia-500/30";
  }
  return "bg-sky-500/15 text-sky-300 border-sky-500/30";
}

function fmtTs(iso: string | null): string {
  if (!iso) return "—";
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

function reasonText(reason: string): string {
  return REASON_LABEL[reason] ?? reason;
}

export function SetupRejections() {
  const [windowHours, setWindowHours] = useState(24);
  const [venue, setVenue] = useState("");
  const [reason, setReason] = useState("");
  const [symbol, setSymbol] = useState("");

  const listQuery = useQuery({
    queryKey: ["v2", "setup-rejections", { windowHours, venue, reason, symbol }],
    queryFn: () => {
      const p = new URLSearchParams();
      p.set("since_hours", String(windowHours));
      p.set("limit", "200");
      if (venue) p.set("venue", venue);
      if (reason) p.set("reason", reason);
      if (symbol) p.set("symbol", symbol);
      return apiFetch<RejectionFeed>(`/v2/setup-rejections?${p.toString()}`);
    },
    refetchInterval: 10_000,
  });

  const summaryQuery = useQuery({
    queryKey: ["v2", "setup-rejections", "summary", { windowHours, venue }],
    queryFn: () => {
      const p = new URLSearchParams();
      p.set("since_hours", String(windowHours));
      if (venue) p.set("venue", venue);
      return apiFetch<RejectionSummary>(
        `/v2/setup-rejections/summary?${p.toString()}`,
      );
    },
    refetchInterval: 10_000,
  });

  const entries = useMemo(
    () => listQuery.data?.entries ?? [],
    [listQuery.data],
  );
  const buckets = summaryQuery.data?.by_reason ?? [];
  const total = summaryQuery.data?.total ?? 0;

  return (
    <div className="space-y-4">
      <div className="flex items-baseline justify-between">
        <div className="text-xs uppercase tracking-wide text-zinc-500">
          Confluence Inspector — {entries.length} rejection
          {entries.length === 1 ? "" : "s"}
        </div>
        <div className="text-xs text-zinc-500">
          {listQuery.data
            ? `Generated at ${fmtTs(listQuery.data.generated_at)}`
            : ""}
        </div>
      </div>

      <div className="flex flex-wrap gap-2 text-xs">
        <select
          value={windowHours}
          onChange={(e) => setWindowHours(Number(e.target.value))}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          {WINDOWS.map((w) => (
            <option key={w.hours} value={w.hours}>
              {w.label}
            </option>
          ))}
        </select>
        <select
          value={venue}
          onChange={(e) => setVenue(e.target.value)}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          {VENUES.map((v) => (
            <option key={v} value={v}>
              {v || "all venues"}
            </option>
          ))}
        </select>
        <input
          type="text"
          placeholder="symbol filter"
          value={symbol}
          onChange={(e) => setSymbol(e.target.value.toUpperCase())}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 font-mono text-zinc-200 placeholder:text-zinc-600"
          style={{ width: 140 }}
        />
        {reason && (
          <button
            type="button"
            onClick={() => setReason("")}
            className="rounded border border-zinc-700 bg-zinc-800/60 px-2 py-1 text-zinc-300 hover:bg-zinc-700/60"
          >
            clear reason: {reasonText(reason)} ✕
          </button>
        )}
      </div>

      {/* Summary chips */}
      <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
        <div className="mb-2 flex items-baseline gap-3">
          <span className="text-[10px] uppercase tracking-wide text-zinc-500">
            By reason (last {windowHours}h)
          </span>
          <span className="text-xs text-zinc-400">total: {total}</span>
        </div>
        {buckets.length === 0 ? (
          <div className="text-xs text-zinc-500">
            No rejections in this window.
          </div>
        ) : (
          <div className="flex flex-wrap gap-1.5">
            {buckets.map((b) => (
              <button
                key={b.reason}
                type="button"
                onClick={() =>
                  setReason(reason === b.reason ? "" : b.reason)
                }
                className={`rounded border px-2 py-1 text-[11px] transition ${reasonBadge(
                  b.reason,
                )} ${
                  reason === b.reason
                    ? "ring-2 ring-yellow-400/60"
                    : "hover:brightness-125"
                }`}
                title="Click to filter the table by this reason"
              >
                <span>{reasonText(b.reason)}</span>
                <span className="ml-1.5 font-mono font-semibold">{b.n}</span>
              </button>
            ))}
          </div>
        )}
      </div>

      {listQuery.isLoading && (
        <div className="text-sm text-zinc-400">Loading rejections…</div>
      )}
      {listQuery.isError && (
        <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
          Failed to load rejections:{" "}
          {(listQuery.error as Error).message}
        </div>
      )}

      {!listQuery.isLoading && entries.length === 0 && (
        <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 px-4 py-6 text-sm text-zinc-500">
          No rejections match the current filter.
        </div>
      )}

      {entries.length > 0 && (
        <div className="overflow-x-auto rounded-lg border border-zinc-800 bg-zinc-900/40">
          <table className="min-w-full">
            <thead className="bg-zinc-900/80 text-[10px] uppercase tracking-wide text-zinc-500">
              <tr>
                <th className="px-2 py-2 text-left">Time</th>
                <th className="px-2 py-2 text-left">Venue</th>
                <th className="px-2 py-2 text-left">Symbol</th>
                <th className="px-2 py-2 text-left">TF</th>
                <th className="px-2 py-2 text-left">Profile</th>
                <th className="px-2 py-2 text-left">Dir</th>
                <th className="px-2 py-2 text-left">Reason</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((r) => (
                <tr
                  key={r.id}
                  className="border-b border-zinc-800/60 text-xs hover:bg-zinc-800/30"
                >
                  <td className="px-2 py-1.5 font-mono text-zinc-400">
                    {fmtTs(r.created_at)}
                  </td>
                  <td className="px-2 py-1.5 text-zinc-400">
                    {r.venue_class}
                  </td>
                  <td className="px-2 py-1.5 font-mono text-zinc-200">
                    {r.symbol}
                  </td>
                  <td className="px-2 py-1.5 text-zinc-500">{r.timeframe}</td>
                  <td className="px-2 py-1.5 uppercase text-zinc-300">
                    {r.profile}
                  </td>
                  <td
                    className={`px-2 py-1.5 uppercase ${
                      r.direction === "long"
                        ? "text-emerald-300"
                        : r.direction === "short"
                          ? "text-red-300"
                          : "text-zinc-400"
                    }`}
                  >
                    {r.direction}
                  </td>
                  <td className="px-2 py-1.5">
                    <span
                      className={`rounded border px-1.5 py-0.5 text-[10px] ${reasonBadge(
                        r.reject_reason,
                      )}`}
                    >
                      {reasonText(r.reject_reason)}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
