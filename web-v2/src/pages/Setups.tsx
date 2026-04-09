import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { apiFetch } from "../lib/api";
import type {
  SetupEntry,
  SetupEventsResponse,
  SetupFeed,
} from "../lib/types";

// Static option lists — data table over inline JSX so adding a new
// venue/profile/state is a one-line change.
const VENUES = ["", "crypto", "bist"];
const PROFILES = ["", "t", "q", "d"];
const STATES = ["", "armed", "live", "trailing", "closed", "rejected"];

const STATE_BADGE: Record<string, string> = {
  armed: "bg-sky-500/15 text-sky-300 border-sky-500/30",
  live: "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  trailing: "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  closed: "bg-zinc-700/40 text-zinc-400 border-zinc-600/40",
  rejected: "bg-red-500/15 text-red-300 border-red-500/30",
};

const PROFILE_BADGE: Record<string, string> = {
  t: "bg-orange-500/15 text-orange-300 border-orange-500/30",
  q: "bg-blue-500/15 text-blue-300 border-blue-500/30",
  d: "bg-purple-500/15 text-purple-300 border-purple-500/30",
};

const DIRECTION_COLOR: Record<string, string> = {
  long: "text-emerald-300",
  short: "text-red-300",
  neutral: "text-zinc-400",
};

function fmtTs(iso: string | null): string {
  if (!iso) return "—";
  return iso.replace("T", " ").replace(/\.\d+Z$/, "Z");
}

function fmtNum(v: number | null, digits = 2): string {
  if (v === null || v === undefined) return "—";
  return v.toFixed(digits);
}

function badge(map: Record<string, string>, key: string): string {
  return map[key] ?? "bg-zinc-800/40 text-zinc-300 border-zinc-700";
}

function SetupRow({
  s,
  onSelect,
  selected,
}: {
  s: SetupEntry;
  onSelect: (id: string) => void;
  selected: boolean;
}) {
  return (
    <tr
      onClick={() => onSelect(s.id)}
      className={`cursor-pointer border-b border-zinc-800/60 text-xs hover:bg-zinc-800/30 ${
        selected ? "bg-emerald-500/5" : ""
      }`}
    >
      <td className="px-2 py-1.5 font-mono text-zinc-300">{s.symbol}</td>
      <td className="px-2 py-1.5 text-zinc-500">{s.timeframe}</td>
      <td className="px-2 py-1.5">
        <span
          className={`rounded border px-1.5 py-0.5 text-[10px] uppercase ${badge(PROFILE_BADGE, s.profile)}`}
        >
          {s.profile}
        </span>
      </td>
      <td className="px-2 py-1.5">
        <span
          className={`rounded border px-1.5 py-0.5 text-[10px] uppercase ${badge(STATE_BADGE, s.state)}`}
        >
          {s.state}
        </span>
      </td>
      <td className={`px-2 py-1.5 uppercase ${DIRECTION_COLOR[s.direction] ?? "text-zinc-400"}`}>
        {s.direction}
      </td>
      <td className="px-2 py-1.5 text-right font-mono text-zinc-200">
        {fmtNum(s.entry_price, 4)}
      </td>
      <td className="px-2 py-1.5 text-right font-mono text-zinc-400">
        {fmtNum(s.koruma, 4)}
      </td>
      <td className="px-2 py-1.5 text-right font-mono text-zinc-400">
        {fmtNum(s.target_ref, 4)}
      </td>
      <td className="px-2 py-1.5 text-right font-mono text-zinc-400">
        {fmtNum(s.risk_pct, 2)}
      </td>
      <td className="px-2 py-1.5 text-zinc-600">{fmtTs(s.updated_at)}</td>
    </tr>
  );
}

function SetupDetail({ id }: { id: string }) {
  const query = useQuery({
    queryKey: ["v2", "setup", id, "events"],
    queryFn: () => apiFetch<SetupEventsResponse>(`/v2/setups/${id}/events`),
    refetchInterval: 5_000,
  });

  if (query.isLoading) {
    return <div className="text-xs text-zinc-500">Loading events…</div>;
  }
  if (query.isError) {
    return (
      <div className="text-xs text-red-300">
        {(query.error as Error).message}
      </div>
    );
  }
  const events = query.data!.events;
  if (events.length === 0) {
    return <div className="text-xs text-zinc-500">No events.</div>;
  }
  return (
    <div className="space-y-2">
      {events.map((ev) => (
        <div
          key={ev.id}
          className="rounded border border-zinc-800 bg-zinc-900/40 p-2 text-xs"
        >
          <div className="flex items-baseline justify-between">
            <span className="font-mono text-zinc-200">{ev.event_type}</span>
            <span className="text-zinc-500">{fmtTs(ev.created_at)}</span>
          </div>
          <div className="mt-1 flex gap-2 text-[10px] text-zinc-500">
            <span>delivery: {ev.delivery_state}</span>
            {ev.delivered_at && <span>at {fmtTs(ev.delivered_at)}</span>}
            {ev.retries > 0 && <span>retries: {ev.retries}</span>}
          </div>
          {ev.payload != null && (
            <pre className="mt-1 overflow-x-auto text-[10px] text-zinc-400">
              {JSON.stringify(ev.payload, null, 2)}
            </pre>
          )}
        </div>
      ))}
    </div>
  );
}

export function Setups() {
  const [venue, setVenue] = useState("");
  const [profile, setProfile] = useState("");
  const [state, setState] = useState("");
  const [selected, setSelected] = useState<string | null>(null);

  const query = useQuery({
    queryKey: ["v2", "setups", { venue, profile, state }],
    queryFn: () => {
      const params = new URLSearchParams();
      if (venue) params.set("venue", venue);
      if (profile) params.set("profile", profile);
      if (state) params.set("state", state);
      params.set("limit", "200");
      return apiFetch<SetupFeed>(`/v2/setups?${params.toString()}`);
    },
    refetchInterval: 5_000,
  });

  const entries = useMemo(() => query.data?.entries ?? [], [query.data]);
  const selectedEntry = useMemo(
    () => entries.find((e) => e.id === selected) ?? null,
    [entries, selected],
  );

  return (
    <div className="space-y-4">
      <div className="flex items-baseline justify-between">
        <div className="text-xs uppercase tracking-wide text-zinc-500">
          Setup Engine — {entries.length} setup
        </div>
        <div className="text-xs text-zinc-500">
          {query.data ? `Generated at ${fmtTs(query.data.generated_at)}` : ""}
        </div>
      </div>

      <div className="flex flex-wrap gap-2 text-xs">
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
        <select
          value={profile}
          onChange={(e) => setProfile(e.target.value)}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          {PROFILES.map((p) => (
            <option key={p} value={p}>
              {p ? `profile ${p.toUpperCase()}` : "all profiles"}
            </option>
          ))}
        </select>
        <select
          value={state}
          onChange={(e) => setState(e.target.value)}
          className="rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-zinc-200"
        >
          {STATES.map((s) => (
            <option key={s} value={s}>
              {s || "all states"}
            </option>
          ))}
        </select>
      </div>

      {query.isLoading && (
        <div className="text-sm text-zinc-400">Loading setups…</div>
      )}
      {query.isError && (
        <div className="rounded border border-red-800 bg-red-950/40 p-4 text-sm text-red-300">
          Failed to load setups: {(query.error as Error).message}
        </div>
      )}

      {!query.isLoading && entries.length === 0 && (
        <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 px-4 py-6 text-sm text-zinc-500">
          No setups match the current filter.
        </div>
      )}

      {entries.length > 0 && (
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
          <div className="lg:col-span-2 overflow-x-auto rounded-lg border border-zinc-800 bg-zinc-900/40">
            <table className="min-w-full">
              <thead className="bg-zinc-900/80 text-[10px] uppercase tracking-wide text-zinc-500">
                <tr>
                  <th className="px-2 py-2 text-left">Symbol</th>
                  <th className="px-2 py-2 text-left">TF</th>
                  <th className="px-2 py-2 text-left">Profile</th>
                  <th className="px-2 py-2 text-left">State</th>
                  <th className="px-2 py-2 text-left">Dir</th>
                  <th className="px-2 py-2 text-right">Entry</th>
                  <th className="px-2 py-2 text-right">Koruma</th>
                  <th className="px-2 py-2 text-right">Target</th>
                  <th className="px-2 py-2 text-right">Risk%</th>
                  <th className="px-2 py-2 text-left">Updated</th>
                </tr>
              </thead>
              <tbody>
                {entries.map((s) => (
                  <SetupRow
                    key={s.id}
                    s={s}
                    onSelect={setSelected}
                    selected={selected === s.id}
                  />
                ))}
              </tbody>
            </table>
          </div>

          <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-3">
            {selectedEntry ? (
              <>
                <div className="mb-3 border-b border-zinc-800 pb-2">
                  <div className="text-sm font-semibold text-zinc-100">
                    {selectedEntry.symbol}{" "}
                    <span className="text-xs text-zinc-500">
                      · {selectedEntry.timeframe} · profile{" "}
                      {selectedEntry.profile.toUpperCase()}
                    </span>
                  </div>
                  <div className="mt-1 flex gap-2 text-[10px] text-zinc-500">
                    <span>{selectedEntry.exchange}</span>
                    <span>{selectedEntry.venue_class}</span>
                    {selectedEntry.alt_type && (
                      <span>alt: {selectedEntry.alt_type}</span>
                    )}
                  </div>
                  <div className="mt-2 grid grid-cols-2 gap-x-3 gap-y-1 text-[11px]">
                    <div className="text-zinc-500">Entry</div>
                    <div className="text-right font-mono text-zinc-200">
                      {fmtNum(selectedEntry.entry_price, 4)}
                    </div>
                    <div className="text-zinc-500">Initial SL</div>
                    <div className="text-right font-mono text-zinc-300">
                      {fmtNum(selectedEntry.entry_sl, 4)}
                    </div>
                    <div className="text-zinc-500">Koruma (trailing)</div>
                    <div className="text-right font-mono text-zinc-300">
                      {fmtNum(selectedEntry.koruma, 4)}
                    </div>
                    <div className="text-zinc-500">Target ref</div>
                    <div className="text-right font-mono text-zinc-300">
                      {fmtNum(selectedEntry.target_ref, 4)}
                    </div>
                    <div className="text-zinc-500">Risk %</div>
                    <div className="text-right font-mono text-zinc-300">
                      {fmtNum(selectedEntry.risk_pct, 2)}
                    </div>
                    {selectedEntry.close_reason && (
                      <>
                        <div className="text-zinc-500">Close</div>
                        <div className="text-right font-mono text-zinc-300">
                          {selectedEntry.close_reason} @{" "}
                          {fmtNum(selectedEntry.close_price, 4)}
                        </div>
                      </>
                    )}
                  </div>
                </div>
                <div className="mb-2 text-[10px] uppercase tracking-wide text-zinc-500">
                  Timeline
                </div>
                <SetupDetail id={selectedEntry.id} />
              </>
            ) : (
              <div className="text-xs text-zinc-500">
                Select a setup to view its event timeline.
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
